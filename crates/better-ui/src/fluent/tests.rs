//! The kit's logic that is not drawing: tokens, state tables, keyboard
//! rules, sorting, and the value models. Each WinUI rule a test asserts names
//! the source file it was read from.

use super::auto_suggest::{SuggestOutcome, list_open, suggest_key};
use super::badge::{InfoBadge, MAX_SHOWN_COUNT};
use super::breadcrumb::{clicked, fold};
use super::button::{ButtonKind, colors};
use super::chrome::{
    CLOSE_RED, CaptionButton, back_tooltip, caption_buttons, caption_colors, is_back_shortcut,
};
use super::command_bar::{fit_count, split_primary};
use super::dialog::{DefaultButton, DialogButtons, DialogResult};
use super::hyperlink::link_color;
use super::info_bar::{ErrorReport, Severity, severity_look};
use super::list::{
    ListItem, SortState, TILE_BODY_HEIGHT, TILE_BODY_WIDTH, TILE_HEIGHT, TILE_ICON, TILE_WIDTH,
    compare_cells, list_step, move_item, row_fill, row_icon_size, sorted_order,
};
use super::menu::{EntryShape, MenuAction, MenuEntry, MenuItem, MenuOpen, MenuPath, shapes};
use super::navigation::{
    NavEntry, NavItem, SearchSlot, content_offset, floats_over_content, is_menu_key, item_fill,
    nav_step, pane_width, search_slot, toggle_tooltip,
};
use super::progress::{fraction, indeterminate_arc, indeterminate_segment};
use super::selection::{ComboOutcome, SliderModel, combo_key};
use super::settings_card::card_fill;
use super::tabs::{SelectorItem, TabOutcome, after_close, selector_step, tab_key};
use super::text_box::{NumberModel, field_colors, multi_line_height};
use super::theme::{Appearance, config_home};
use super::toggle::{
    CheckState, RadioOption, mark_colors, radio_step, switch_colors, switch_toggles,
};
use super::words::{self, Locale};
use super::*;

fn byte(channel: f32) -> u32 {
    (channel * 255.0).round() as u32
}

fn argb_of(color: Rgba) -> u32 {
    (byte(color.a) << 24) | (byte(color.r) << 16) | (byte(color.g) << 8) | byte(color.b)
}

#[test]
fn hex_and_argb_read_each_channel() {
    assert_eq!(argb_of(hex(0x005fb8)), 0xff005fb8);
    assert_eq!(argb_of(argb(0x9e000000)), 0x9e000000);
}

/// `Common_themeresources_any.xaml` at e38939f, the Light and Default
/// (dark) dictionaries, row by row. A change here is a change to the design
/// baseline in `DESIGN.md`, not a drive-by edit.
#[test]
fn the_palettes_are_winui_theme_resources() {
    let rows: [(&str, Rgba, Rgba, u32, u32); 22] = [
        (
            "SolidBackgroundFillColorBase",
            LIGHT.window_background,
            DARK.window_background,
            0xfff3f3f3,
            0xff202020,
        ),
        (
            "CardBackgroundFillColorDefault",
            LIGHT.card,
            DARK.card,
            0xb3ffffff,
            0x0dffffff,
        ),
        (
            "LayerFillColorDefault",
            LIGHT.layer,
            DARK.layer,
            0x80ffffff,
            0x4c3a3a3a,
        ),
        (
            "TextFillColorPrimary",
            LIGHT.text_primary,
            DARK.text_primary,
            0xe4000000,
            0xffffffff,
        ),
        (
            "TextFillColorSecondary",
            LIGHT.text_secondary,
            DARK.text_secondary,
            0x9e000000,
            0xc5ffffff,
        ),
        (
            "TextFillColorTertiary",
            LIGHT.text_tertiary,
            DARK.text_tertiary,
            0x72000000,
            0x87ffffff,
        ),
        (
            "TextFillColorDisabled",
            LIGHT.text_disabled,
            DARK.text_disabled,
            0x5c000000,
            0x5dffffff,
        ),
        (
            "TextOnAccentFillColorPrimary",
            LIGHT.text_on_accent,
            DARK.text_on_accent,
            0xffffffff,
            0xff000000,
        ),
        (
            "ControlFillColorDefault",
            LIGHT.control_fill,
            DARK.control_fill,
            0xb3ffffff,
            0x0fffffff,
        ),
        (
            "ControlFillColorSecondary",
            LIGHT.control_fill_secondary,
            DARK.control_fill_secondary,
            0x80f9f9f9,
            0x15ffffff,
        ),
        (
            "ControlFillColorTertiary",
            LIGHT.control_fill_tertiary,
            DARK.control_fill_tertiary,
            0x4df9f9f9,
            0x08ffffff,
        ),
        (
            "ControlFillColorInputActive",
            LIGHT.control_fill_input_active,
            DARK.control_fill_input_active,
            0xffffffff,
            0xb31e1e1e,
        ),
        (
            "SubtleFillColorSecondary",
            LIGHT.subtle_fill,
            DARK.subtle_fill,
            0x09000000,
            0x0fffffff,
        ),
        (
            "SubtleFillColorTertiary",
            LIGHT.subtle_fill_tertiary,
            DARK.subtle_fill_tertiary,
            0x06000000,
            0x0affffff,
        ),
        (
            "ControlStrokeColorDefault",
            LIGHT.stroke,
            DARK.stroke,
            0x0f000000,
            0x12ffffff,
        ),
        (
            "ControlStrongStrokeColorDefault",
            LIGHT.strong_stroke,
            DARK.strong_stroke,
            0x72000000,
            0x8bffffff,
        ),
        (
            "CardStrokeColorDefault",
            LIGHT.card_stroke,
            DARK.card_stroke,
            0x0f000000,
            0x19000000,
        ),
        (
            "FocusStrokeColorOuter",
            LIGHT.focus_outer,
            DARK.focus_outer,
            0xe4000000,
            0xffffffff,
        ),
        (
            "FocusStrokeColorInner",
            LIGHT.focus_inner,
            DARK.focus_inner,
            0xb3ffffff,
            0xb3000000,
        ),
        (
            "SystemFillColorCritical",
            LIGHT.critical,
            DARK.critical,
            0xffc42b1c,
            0xffff99a4,
        ),
        (
            "SystemFillColorSuccess",
            LIGHT.success,
            DARK.success,
            0xff0f7b0f,
            0xff6ccb5f,
        ),
        (
            "SystemFillColorCaution",
            LIGHT.caution,
            DARK.caution,
            0xff9d5d00,
            0xfffce100,
        ),
    ];
    for (name, light, dark, light_value, dark_value) in rows {
        assert_eq!(argb_of(light), light_value, "light {name}");
        assert_eq!(argb_of(dark), dark_value, "dark {name}");
    }
    // AcrylicBrush_themeresources.xaml: the flyout fill's fallback colors.
    assert_eq!(argb_of(LIGHT.flyout), 0xfff9f9f9);
    assert_eq!(argb_of(DARK.flyout), 0xff2c2c2c);
    // The accent is the operating system's; these are Windows 11's default
    // blue shades (`SystemAccentColorDark1` and `Light2`).
    assert_eq!(argb_of(LIGHT.accent), 0xff005fb8);
    assert_eq!(argb_of(DARK.accent), 0xff60cdff);
}

#[test]
fn the_appearance_picks_its_palette() {
    assert_eq!(palette_for(true), &DARK);
    assert_eq!(palette_for(false), &LIGHT);
    assert!(DARK.is_dark());
    assert!(!LIGHT.is_dark());
}

#[test]
fn the_type_ramp_is_five_sizes_and_never_below_twelve() {
    for style in [BODY, BODY_STRONG, SUBTITLE, TITLE] {
        assert!(style.size >= 14.0);
        assert!(style.line_height > style.size);
    }
    assert_eq!((CAPTION.size, CAPTION.line_height), (12.0, 16.0));
    assert_eq!((BODY.size, BODY.line_height), (14.0, 20.0));
    assert_eq!((SUBTITLE.size, SUBTITLE.line_height), (20.0, 28.0));
    assert_eq!((TITLE.size, TITLE.line_height), (28.0, 36.0));
    assert_eq!((CODE.size, CODE.line_height), (13.0, 20.0));
}

/// No control's source sets a text size outside the ramp: every
/// `text_size(px(N))` in the kit is 12, 14, 20, or 28, or the 13px code
/// style `DESIGN.md` gives paths and command output.
#[test]
fn no_control_draws_text_outside_the_ramp() {
    let sources = [
        include_str!("auto_suggest.rs"),
        include_str!("badge.rs"),
        include_str!("hyperlink.rs"),
        include_str!("breadcrumb.rs"),
        include_str!("button.rs"),
        include_str!("chip.rs"),
        include_str!("chrome.rs"),
        include_str!("command_bar.rs"),
        include_str!("dialog.rs"),
        include_str!("info_bar.rs"),
        include_str!("lanes.rs"),
        include_str!("list.rs"),
        include_str!("menu.rs"),
        include_str!("navigation.rs"),
        include_str!("progress.rs"),
        include_str!("selection.rs"),
        include_str!("settings_card.rs"),
        include_str!("tabs.rs"),
        include_str!("text_box.rs"),
        include_str!("toggle.rs"),
    ];
    for source in sources {
        for (at, _) in source.match_indices(".text_size(px(") {
            let rest = &source[at + ".text_size(px(".len()..];
            let value: String = rest.chars().take_while(|c| *c != ')').collect();
            assert!(
                ["12.0", "14.0", "20.0", "28.0", "CODE.size"].contains(&value.as_str()),
                "text size {value} is not on the type ramp"
            );
        }
        assert!(!source.contains("text_xs()"), "a control uses text_xs");
    }
}

#[test]
fn spacing_is_on_the_four_pixel_grid() {
    for step in [
        spacing::XS,
        spacing::SM,
        spacing::MD,
        spacing::LG,
        spacing::XL,
    ] {
        assert_eq!(f32::from(step) % 4.0, 0.0);
    }
}

#[test]
fn a_gallery_state_forces_one_visual_state() {
    assert_eq!(ShownState::Live.forced(), None);
    assert_eq!(ShownState::Rest.forced(), Some(Interaction::Rest));
    assert_eq!(
        ShownState::PointerOver.forced(),
        Some(Interaction::PointerOver)
    );
    assert_eq!(ShownState::Pressed.forced(), Some(Interaction::Pressed));
    assert_eq!(ShownState::Focused.forced(), Some(Interaction::Rest));
    assert!(ShownState::Focused.shows_focus());
    assert!(!ShownState::Rest.shows_focus());
    assert_eq!(
        resolve(true, ShownState::PointerOver),
        Some(Interaction::Disabled)
    );
    assert_eq!(resolve(false, ShownState::Live), None);
    assert_eq!(ShownState::ALL.len(), 5);
}

#[test]
fn keys_parse_with_their_modifiers() {
    assert_eq!(Key::parse("enter", false, false, false), Some(Key::Enter));
    assert_eq!(Key::parse("space", false, false, false), Some(Key::Space));
    assert_eq!(
        Key::parse("down", false, true, false),
        Some(Key::OpenDropDown)
    );
    assert_eq!(
        Key::parse("f4", false, false, false),
        Some(Key::OpenDropDown)
    );
    assert_eq!(Key::parse("tab", true, false, false), Some(Key::NextTab));
    assert_eq!(Key::parse("tab", true, false, true), Some(Key::PreviousTab));
    assert_eq!(Key::parse("w", true, false, false), Some(Key::CloseTab));
    assert_eq!(Key::parse("f4", true, false, false), Some(Key::CloseTab));
    // Ctrl or Alt with any other key is the window's shortcut, not ours.
    assert_eq!(Key::parse("enter", true, false, false), None);
    assert_eq!(Key::parse("a", false, false, false), None);
    assert!(Key::Enter.invokes() && Key::Space.invokes());
    assert!(!Key::Escape.invokes());
}

#[test]
fn stepping_skips_and_stops_or_wraps() {
    let skip = |i: usize| i == 1;
    assert_eq!(step_index(Some(0), 4, Key::Down, false, skip), Some(2));
    assert_eq!(step_index(Some(3), 4, Key::Down, false, skip), None);
    assert_eq!(step_index(Some(3), 4, Key::Down, true, skip), Some(0));
    assert_eq!(step_index(Some(0), 4, Key::Up, true, skip), Some(3));
    assert_eq!(step_index(Some(2), 4, Key::Home, false, skip), Some(0));
    assert_eq!(step_index(None, 4, Key::Down, false, skip), Some(0));
    assert_eq!(step_index(None, 4, Key::Up, false, skip), Some(3));
    assert_eq!(step_index(Some(0), 0, Key::Down, true, skip), None);
    assert_eq!(step_index(None, 2, Key::Down, true, |_| true), None);
}

#[test]
fn button_colors_follow_button_themeresources() {
    let accent = colors(ButtonKind::Accent, false, &LIGHT);
    assert_eq!(accent.rest.background, LIGHT.accent);
    assert_eq!(accent.pointer_over.background, LIGHT.accent_secondary);
    assert_eq!(accent.pressed.background, LIGHT.accent_tertiary);
    assert_eq!(accent.pressed.foreground, LIGHT.text_on_accent_secondary);
    assert_eq!(accent.disabled.background, LIGHT.accent_disabled);
    let standard = colors(ButtonKind::Standard, false, &DARK);
    assert_eq!(standard.rest.background, DARK.control_fill);
    assert_eq!(standard.rest.elevation, Some(DARK.stroke_secondary));
    assert_eq!(standard.pressed.foreground, DARK.text_secondary);
    assert_eq!(standard.pressed.elevation, None);
    assert_eq!(standard.disabled.foreground, DARK.text_disabled);
    let subtle = colors(ButtonKind::Subtle, false, &LIGHT);
    assert_eq!(subtle.rest.background.a, 0.0);
    assert_eq!(subtle.pointer_over.background, LIGHT.subtle_fill);
    // ToggleButton_themeresources.xaml: checked is the accent button.
    assert_eq!(colors(ButtonKind::Standard, true, &LIGHT), accent);
}

#[test]
fn a_check_box_toggles_as_winui_does() {
    // ToggleButton_Partial.cpp, OnToggleImpl.
    assert_eq!(CheckState::Unchecked.toggled(false), CheckState::Checked);
    assert_eq!(CheckState::Checked.toggled(false), CheckState::Unchecked);
    assert_eq!(
        CheckState::Indeterminate.toggled(false),
        CheckState::Unchecked
    );
    assert_eq!(CheckState::Checked.toggled(true), CheckState::Indeterminate);
    assert_eq!(
        CheckState::Indeterminate.toggled(true),
        CheckState::Unchecked
    );
    assert_eq!(CheckState::summarize([true, true]), CheckState::Checked);
    assert_eq!(
        CheckState::summarize([true, false]),
        CheckState::Indeterminate
    );
    assert_eq!(CheckState::summarize([false, false]), CheckState::Unchecked);
    assert_eq!(CheckState::summarize([]), CheckState::Unchecked);
}

#[test]
fn marks_and_switches_use_the_checkbox_tables() {
    let off = mark_colors(false, Interaction::Rest, &LIGHT);
    assert_eq!(off.stroke, LIGHT.strong_stroke);
    assert_eq!(off.fill, LIGHT.control_alt_fill);
    let on = mark_colors(true, Interaction::PointerOver, &DARK);
    assert_eq!(on.fill, DARK.accent_secondary);
    let switch = switch_colors(true, Interaction::Rest, &LIGHT);
    assert_eq!(switch.track, LIGHT.accent);
    assert_eq!(switch.knob, LIGHT.text_on_accent);
    let switch_off = switch_colors(false, Interaction::Disabled, &DARK);
    assert_eq!(switch_off.knob, DARK.text_disabled);
    // ToggleSwitch::HandlesKey: Space only.
    assert!(switch_toggles(Key::Space));
    assert!(!switch_toggles(Key::Enter));
}

#[test]
fn a_radio_group_moves_with_the_arrows_and_stops_at_its_ends() {
    let options = vec![
        RadioOption::new("a"),
        RadioOption::new("b").disabled(true),
        RadioOption::new("c"),
    ];
    assert_eq!(radio_step(Some(0), &options, Key::Down), Some(2));
    assert_eq!(radio_step(Some(2), &options, Key::Right), None);
    assert_eq!(radio_step(Some(2), &options, Key::Up), Some(0));
    assert_eq!(radio_step(Some(0), &options, Key::Enter), None);
}

#[test]
fn text_fields_show_focus_with_a_two_pixel_accent_edge() {
    let rest = field_colors(Interaction::Rest, false, &LIGHT);
    assert_eq!(
        (rest.bottom, f32::from(rest.bottom_width)),
        (LIGHT.strong_stroke, 1.0)
    );
    let focused = field_colors(Interaction::Rest, true, &LIGHT);
    assert_eq!(
        (focused.bottom, f32::from(focused.bottom_width)),
        (LIGHT.accent, 2.0)
    );
    assert_eq!(focused.background, LIGHT.control_fill_input_active);
    let disabled = field_colors(Interaction::Disabled, true, &DARK);
    assert_eq!(disabled.foreground, DARK.text_disabled);
}

#[test]
fn a_number_box_steps_validates_and_formats() {
    let model = NumberModel {
        minimum: 0.0,
        maximum: 100.0,
        small_change: 1.0,
        large_change: 10.0,
        wrap: false,
    };
    // NumberBox.cpp, OnNumberBoxKeyDown and StepValue.
    assert_eq!(model.key(5.0, Key::Up), Some(6.0));
    assert_eq!(model.key(5.0, Key::PageDown), Some(0.0));
    assert_eq!(model.key(99.0, Key::PageUp), Some(100.0));
    assert_eq!(model.key(5.0, Key::Left), None);
    let wrapping = NumberModel {
        wrap: true,
        ..model
    };
    assert_eq!(wrapping.key(100.0, Key::Up), Some(0.0));
    assert_eq!(wrapping.key(0.0, Key::Down), Some(100.0));
    assert!(model.step(f64::NAN, 1.0).is_nan());
    // InvalidInputOverwritten: bad text keeps the last value; out of range
    // is pulled back in.
    assert_eq!(model.validate("abc", 7.0), 7.0);
    assert_eq!(model.validate(" 250 ", 7.0), 100.0);
    assert_eq!(model.validate("12.5", 7.0), 12.5);
    assert_eq!(NumberModel::format(12.0), "12");
    assert_eq!(NumberModel::format(12.5), "12.5");
    assert_eq!(NumberModel::format(-0.0), "0");
    assert_eq!(NumberModel::format(f64::NAN), "");
}

#[test]
fn a_combo_box_follows_the_winui_keyboard() {
    let none = |_: usize| false;
    assert_eq!(
        combo_key(false, Some(1), None, 4, Key::Enter, none),
        ComboOutcome::Open(Some(1))
    );
    assert_eq!(
        combo_key(false, Some(1), None, 4, Key::OpenDropDown, none),
        ComboOutcome::Open(Some(1))
    );
    assert_eq!(
        combo_key(false, Some(1), None, 4, Key::Down, none),
        ComboOutcome::Select(2)
    );
    assert_eq!(
        combo_key(false, Some(3), None, 4, Key::Down, none),
        ComboOutcome::Ignored
    );
    assert_eq!(
        combo_key(true, Some(1), Some(1), 4, Key::Down, |i| i == 2),
        ComboOutcome::Highlight(3)
    );
    assert_eq!(
        combo_key(true, Some(1), Some(3), 4, Key::Enter, none),
        ComboOutcome::Commit(3)
    );
    assert_eq!(
        combo_key(true, Some(1), Some(3), 4, Key::Escape, none),
        ComboOutcome::Close
    );
    assert_eq!(
        combo_key(true, Some(1), None, 4, Key::Space, none),
        ComboOutcome::Close
    );
}

#[test]
fn a_slider_snaps_and_steps() {
    let model = SliderModel {
        minimum: 0.0,
        maximum: 100.0,
        step: 5.0,
        large_change: 20.0,
    };
    assert_eq!(model.snap(12.0), 10.0);
    assert_eq!(model.snap(13.0), 15.0);
    assert_eq!(model.snap(140.0), 100.0);
    assert_eq!(model.fraction(25.0), 0.25);
    assert_eq!(model.value_at(0.5), 50.0);
    assert_eq!(model.value_at(-1.0), 0.0);
    assert_eq!(model.key(50.0, Key::Right), Some(55.0));
    assert_eq!(model.key(50.0, Key::Down), Some(45.0));
    assert_eq!(model.key(50.0, Key::PageUp), Some(70.0));
    assert_eq!(model.key(50.0, Key::End), Some(100.0));
    assert_eq!(model.key(0.0, Key::Home), None);
    assert_eq!(SliderModel::new(5.0, 5.0).fraction(5.0), 0.0);
}

fn menu() -> Vec<EntryShape> {
    vec![
        EntryShape::Item { enabled: true },
        EntryShape::Item { enabled: false },
        EntryShape::Separator,
        EntryShape::Submenu {
            enabled: true,
            children: vec![
                EntryShape::Item { enabled: true },
                EntryShape::Separator,
                EntryShape::Item { enabled: true },
            ],
        },
        EntryShape::Item { enabled: true },
    ]
}

fn open_menu_state() -> MenuOpen {
    let mut state = MenuOpen::detached();
    state.open_with_highlight(None);
    state
}

#[test]
fn a_menu_wraps_skips_and_opens_submenus() {
    let shapes = menu();
    let mut state = open_menu_state();
    assert_eq!(state.handle_key(&shapes, Key::Down), MenuAction::Moved);
    assert_eq!(state.highlight, Some(0));
    // The disabled item and the separator are skipped.
    state.handle_key(&shapes, Key::Down);
    assert_eq!(state.highlight, Some(3));
    state.handle_key(&shapes, Key::Down);
    assert_eq!(state.highlight, Some(4));
    // Wraps at the end.
    state.handle_key(&shapes, Key::Down);
    assert_eq!(state.highlight, Some(0));
    state.handle_key(&shapes, Key::Up);
    assert_eq!(state.highlight, Some(4));
    state.highlight = Some(3);
    assert_eq!(state.handle_key(&shapes, Key::Right), MenuAction::Moved);
    assert_eq!((state.submenu, state.sub_highlight), (Some(3), Some(0)));
    state.handle_key(&shapes, Key::Down);
    assert_eq!(state.sub_highlight, Some(2));
    assert_eq!(
        state.handle_key(&shapes, Key::Enter),
        MenuAction::Invoke(MenuPath {
            top: 3,
            sub: Some(2)
        })
    );
    assert_eq!(state.handle_key(&shapes, Key::Left), MenuAction::Moved);
    assert_eq!((state.submenu, state.highlight), (None, Some(3)));
    state.highlight = Some(0);
    assert_eq!(
        state.handle_key(&shapes, Key::Space),
        MenuAction::Invoke(MenuPath { top: 0, sub: None })
    );
    // Right on a plain item does nothing; Escape closes the menu.
    assert_eq!(state.handle_key(&shapes, Key::Right), MenuAction::Ignored);
    assert_eq!(state.handle_key(&shapes, Key::Escape), MenuAction::Close);
    assert!(!state.is_open());
}

#[test]
fn escape_in_a_submenu_closes_only_the_submenu() {
    let shapes = menu();
    let mut state = open_menu_state();
    state.highlight = Some(3);
    state.handle_key(&shapes, Key::Enter);
    assert_eq!(state.submenu, Some(3));
    assert_eq!(state.handle_key(&shapes, Key::Escape), MenuAction::Moved);
    assert!(state.is_open());
    assert_eq!(state.submenu, None);
}

#[test]
fn a_dialog_lays_out_its_buttons_and_reads_its_keys() {
    let buttons = DialogButtons {
        primary: Some("Save".into()),
        secondary: None,
        close: Some("Cancel".into()),
        default: DefaultButton::Primary,
    };
    let layout = buttons.layout();
    assert_eq!(layout.len(), 2);
    assert_eq!(layout[0].0, DialogResult::Primary);
    assert!(layout[0].2);
    assert!(!layout[1].2);
    assert_eq!(buttons.key(Key::Enter), Some(DialogResult::Primary));
    assert_eq!(buttons.key(Key::Escape), Some(DialogResult::Close));
    let no_default = DialogButtons {
        default: DefaultButton::None,
        ..buttons.clone()
    };
    assert_eq!(no_default.key(Key::Enter), None);
    // A disabled primary button is not pressed by Enter; Escape still closes.
    assert_eq!(buttons.key_with(Key::Enter, false), None);
    assert_eq!(
        buttons.key_with(Key::Escape, false),
        Some(DialogResult::Close)
    );
    assert_eq!(
        buttons.key_with(Key::Enter, true),
        Some(DialogResult::Primary)
    );
}

#[test]
fn a_list_moves_and_a_row_fills_but_never_outlines() {
    let items = vec![
        ListItem::new("a"),
        ListItem::new("b").disabled(true),
        ListItem::new("c"),
    ];
    assert_eq!(list_step(Some(0), &items, Key::Down), Some(2));
    assert_eq!(list_step(Some(2), &items, Key::Down), None);
    assert_eq!(list_step(None, &items, Key::End), Some(2));
    assert_eq!(row_fill(true, Interaction::Rest, &LIGHT), LIGHT.subtle_fill);
    assert_eq!(row_fill(false, Interaction::Rest, &LIGHT).a, 0.0);
    assert_eq!(
        row_fill(true, Interaction::PointerOver, &DARK),
        DARK.subtle_fill_tertiary
    );
}

#[test]
fn table_headers_sort_naturally_and_reverse_on_a_second_click() {
    let first = SortState::clicked(None, 1);
    assert_eq!(
        first,
        SortState {
            column: 1,
            ascending: true
        }
    );
    let second = SortState::clicked(Some(first), 1);
    assert!(!second.ascending);
    assert!(SortState::clicked(Some(second), 0).ascending);

    use std::cmp::Ordering;
    assert_eq!(compare_cells("file2", "file10"), Ordering::Less);
    assert_eq!(compare_cells("Budget", "archive"), Ordering::Greater);
    assert_eq!(compare_cells("640 KB", "1.2 MB"), Ordering::Less);
    assert_eq!(compare_cells("3.4 GB", "88 KB"), Ordering::Greater);
    assert_eq!(compare_cells("2026-11-02", "2026-09-14"), Ordering::Greater);
    assert_eq!(compare_cells("1,200", "900"), Ordering::Greater);
    assert_eq!(compare_cells("a", "A"), Ordering::Equal);

    let rows: Vec<Vec<gpui::SharedString>> = ["b", "a10", "a2", "A2"]
        .iter()
        .map(|name| vec![(*name).into()])
        .collect();
    let ascending = sorted_order(
        &rows,
        Some(SortState {
            column: 0,
            ascending: true,
        }),
    );
    // Stable: "a2" stays before "A2".
    assert_eq!(ascending, vec![2, 3, 1, 0]);
    let descending = sorted_order(
        &rows,
        Some(SortState {
            column: 0,
            ascending: false,
        }),
    );
    assert_eq!(descending, vec![0, 1, 2, 3]);
    assert_eq!(sorted_order(&rows, None), vec![0, 1, 2, 3]);
}

#[test]
fn the_navigation_pane_skips_headers_and_never_vanishes() {
    let entries = vec![
        NavEntry::Header("Library".into()),
        NavItem::new("Home", gpui_component::IconName::Folder).entry(),
        NavEntry::Separator,
        NavItem::new("Off", gpui_component::IconName::Folder)
            .disabled(true)
            .entry(),
        NavItem::new("Settings", gpui_component::IconName::Settings).entry(),
    ];
    assert_eq!(nav_step(None, &entries, Key::Down), Some(1));
    assert_eq!(nav_step(Some(1), &entries, Key::Down), Some(4));
    assert_eq!(nav_step(Some(4), &entries, Key::Down), None);
    assert_eq!(nav_step(Some(4), &entries, Key::Home), Some(1));
    assert_eq!(f32::from(pane_width(true)), 48.0);
    assert_eq!(f32::from(pane_width(false)), 320.0);
    assert_eq!(
        item_fill(true, Interaction::Rest, &LIGHT),
        LIGHT.subtle_fill
    );
    assert_eq!(toggle_tooltip(Locale::English, true), "Open navigation");
    assert_eq!(
        toggle_tooltip(Locale::TraditionalChinese, false),
        "收合導覽窗格"
    );
}

#[test]
fn tabs_move_wrap_and_close() {
    assert_eq!(tab_key(Some(2), 3, Key::NextTab), TabOutcome::Select(0));
    assert_eq!(tab_key(Some(0), 3, Key::PreviousTab), TabOutcome::Select(2));
    assert_eq!(tab_key(Some(2), 3, Key::Right), TabOutcome::Ignored);
    assert_eq!(tab_key(Some(1), 3, Key::CloseTab), TabOutcome::Close(1));
    assert_eq!(tab_key(None, 3, Key::CloseTab), TabOutcome::Ignored);
    // Closing the selected tab selects the one that slides into its place,
    // or the one before when it was last.
    assert_eq!(after_close(Some(1), 1, 3), Some(1));
    assert_eq!(after_close(Some(2), 2, 3), Some(1));
    assert_eq!(after_close(Some(2), 0, 3), Some(1));
    assert_eq!(after_close(Some(0), 2, 3), Some(0));
    assert_eq!(after_close(Some(0), 0, 1), None);
    let items = vec![
        SelectorItem::new("Recent"),
        SelectorItem::new("Off").disabled(true),
        SelectorItem::new("Shared"),
    ];
    assert_eq!(selector_step(Some(0), &items, Key::Right), Some(2));
    assert_eq!(selector_step(Some(2), &items, Key::Right), None);
}

#[test]
fn breadcrumbs_fold_from_the_start_and_truncate_on_click() {
    assert_eq!(fold(3, 5), (vec![], vec![0, 1, 2]));
    assert_eq!(fold(5, 3), (vec![0, 1, 2], vec![3, 4]));
    assert_eq!(fold(5, 1), (vec![0, 1, 2, 3], vec![4]));
    assert_eq!(fold(0, 3), (vec![], vec![]));
    assert_eq!(clicked(&["a", "b", "c"], 1), vec!["a", "b"]);
    assert_eq!(clicked(&["a"], 5), vec!["a"]);
}

#[test]
fn progress_math_stays_in_range() {
    assert_eq!(fraction(5.0, 0.0, 10.0), 0.5);
    assert_eq!(fraction(15.0, 0.0, 10.0), 1.0);
    assert_eq!(fraction(5.0, 10.0, 10.0), 0.0);
    assert_eq!(fraction(f64::NAN, 0.0, 1.0), 0.0);
    for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let (left, width) = indeterminate_segment(t);
        assert!((0.0..=1.0).contains(&left) && left + width <= 1.0 + f32::EPSILON);
        let (start, sweep) = indeterminate_arc(t);
        assert!((0.0..360.0).contains(&start) && (30.0..=270.0).contains(&sweep));
    }
}

#[test]
fn info_bars_tint_rather_than_paint_in_the_status_color() {
    for p in [&LIGHT, &DARK] {
        let (fill, icon, _) = severity_look(Severity::Error, p);
        assert_eq!(fill, p.critical_background);
        assert_eq!(icon, p.critical);
        assert_ne!(fill, icon);
        let (fill, _, _) = severity_look(Severity::Informational, p);
        assert_eq!(fill, p.attention_background);
    }
}

#[derive(Debug)]
struct Outer(std::io::Error);

impl std::fmt::Display for Outer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("could not read the index")
    }
}

impl std::error::Error for Outer {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

#[test]
fn an_error_report_keeps_the_whole_chain_in_its_details() {
    let error = Outer(std::io::Error::other("permission denied"));
    let report = ErrorReport::from_error("Better Git couldn't open this folder.", &error)
        .advice("Check the folder's permissions.");
    assert_eq!(
        report.summary.as_ref(),
        "Better Git couldn't open this folder."
    );
    assert_eq!(
        report.details.as_ref(),
        "could not read the index\nCaused by: permission denied"
    );
    assert!(report.advice.is_some());
}

#[test]
fn settings_cards_change_fill_only_when_clickable() {
    assert_eq!(
        card_fill(false, Interaction::PointerOver, &LIGHT),
        LIGHT.card
    );
    assert_eq!(
        card_fill(true, Interaction::PointerOver, &LIGHT),
        LIGHT.control_fill_secondary
    );
    assert_eq!(
        card_fill(true, Interaction::Pressed, &DARK),
        DARK.control_fill_tertiary
    );
}

#[test]
fn the_close_caption_button_turns_red() {
    let (fill, glyph) = caption_colors(CaptionButton::Close, Interaction::PointerOver, true, &DARK);
    assert_eq!((fill, glyph), (CLOSE_RED, hex(0xffffff)));
    let (_, idle) = caption_colors(CaptionButton::Minimize, Interaction::Rest, false, &LIGHT);
    assert_eq!(idle, LIGHT.text_tertiary);
    let (fill, _) = caption_colors(
        CaptionButton::Maximize,
        Interaction::PointerOver,
        true,
        &LIGHT,
    );
    assert_eq!(fill, LIGHT.subtle_fill);
}

#[test]
fn a_command_bar_keeps_what_fits() {
    let widths = [px(80.0), px(80.0), px(80.0)];
    assert_eq!(fit_count(&widths, px(240.0), px(40.0)), 3);
    assert_eq!(fit_count(&widths, px(239.0), px(40.0)), 2);
    assert_eq!(fit_count(&widths, px(100.0), px(40.0)), 0);
}

#[test]
fn a_command_bar_moves_the_tail_into_the_overflow_and_never_ends_on_a_divider() {
    // command, separator, command, command, separator, command
    let kinds = [true, false, true, true, false, true];
    assert_eq!(
        split_primary(&kinds, None),
        (vec![0, 1, 2, 3, 4, 5], vec![])
    );
    assert_eq!(split_primary(&kinds, Some(3)), (vec![0, 1, 2, 3], vec![5]));
    assert_eq!(split_primary(&kinds, Some(1)), (vec![0], vec![2, 3, 5]));
    assert_eq!(split_primary(&kinds, Some(0)), (vec![], vec![0, 2, 3, 5]));
}

#[test]
fn a_toggle_menu_item_is_an_ordinary_item_to_the_keyboard() {
    let entries = vec![
        MenuItem::new("Show hidden files").checked(true).entry(),
        MenuItem::new("Folders first").checked(false).entry(),
        MenuEntry::Separator,
        MenuItem::new("Off").checked(true).disabled(true).entry(),
    ];
    assert_eq!(
        shapes(&entries),
        vec![
            EntryShape::Item { enabled: true },
            EntryShape::Item { enabled: true },
            EntryShape::Separator,
            EntryShape::Item { enabled: false },
        ]
    );
    let MenuEntry::Item(item) = &entries[0] else {
        panic!("an item");
    };
    assert_eq!(item.checked, Some(true));
}

#[test]
fn a_navigation_item_with_a_caption_is_two_lines_tall() {
    let plain = NavItem::new("Home", gpui_component::IconName::Folder);
    assert_eq!(plain.height(), size::ROW_HEIGHT);
    let drive = plain.clone().caption("Safe to unplug");
    assert_eq!(drive.height(), size::TWO_LINE_ROW_HEIGHT);
}

#[test]
fn the_menu_key_and_shift_f10_ask_for_a_context_menu() {
    let key = |text: &str| gpui::Keystroke::parse(text).expect("a keystroke");
    assert!(is_menu_key(&key("menu")));
    assert!(is_menu_key(&key("shift-f10")));
    assert!(!is_menu_key(&key("f10")));
    assert!(!is_menu_key(&key("ctrl-shift-f10")));
    assert!(!is_menu_key(&key("enter")));
}

#[test]
fn the_appearance_is_stored_as_a_word_and_defaults_to_the_system() {
    // ADR 0019: system, light, or dark; the system by default, listed first.
    assert_eq!(Appearance::default(), Appearance::System);
    assert_eq!(Appearance::ALL[0], Appearance::System);
    assert!(Appearance::System.follows_desktop());
    assert!(!Appearance::Dark.follows_desktop());
    assert!(!Appearance::Light.follows_desktop());
    for appearance in Appearance::ALL {
        assert_eq!(Appearance::parse(appearance.as_str()), Some(appearance));
    }
    assert_eq!(Appearance::parse(" Light\n"), Some(Appearance::Light));
    assert_eq!(Appearance::parse("blue"), None);
    assert!(Appearance::System.is_dark(gpui::WindowAppearance::Dark));
    assert!(!Appearance::System.is_dark(gpui::WindowAppearance::Light));
    assert!(Appearance::Dark.is_dark(gpui::WindowAppearance::Light));

    let dir = std::env::temp_dir().join(format!("fluent-appearance-{}", std::process::id()));
    let path = dir.join("nested").join("appearance");
    // No file: the person never picked.
    assert_eq!(Appearance::load(&path), Appearance::System);
    Appearance::Light.save(&path).expect("save");
    assert_eq!(Appearance::load(&path), Appearance::Light);
    // A dark someone picked stays dark.
    Appearance::Dark.save(&path).expect("save");
    assert_eq!(Appearance::load(&path), Appearance::Dark);
    std::fs::write(&path, "nonsense").expect("write");
    assert_eq!(Appearance::load(&path), Appearance::System);
    std::fs::remove_dir_all(&dir).expect("clean up");

    assert_eq!(
        config_home(Some("/x/config".into()), Some("/home/a".into())),
        Some("/x/config".into())
    );
    assert_eq!(
        config_home(Some("relative".into()), Some("/home/a".into())),
        Some("/home/a/.config".into())
    );
    assert_eq!(config_home(None, None), None);
}

#[test]
fn shared_words_read_the_same_in_every_window() {
    assert_eq!(Locale::from_tag("zh_TW.UTF-8"), Locale::TraditionalChinese);
    assert_eq!(Locale::from_tag("zh-Hant"), Locale::TraditionalChinese);
    assert_eq!(Locale::from_tag("zh_CN.UTF-8"), Locale::English);
    assert_eq!(Locale::from_tag("en_US"), Locale::English);
    assert_eq!(words::on_off(Locale::English, true), "On");
    assert_eq!(words::on_off(Locale::TraditionalChinese, false), "關閉");
    assert_eq!(words::yes_no(Locale::TraditionalChinese, true), "是");
    assert_eq!(
        words::state(Locale::TraditionalChinese, words::State::Applying),
        "正在套用設定"
    );
    assert_eq!(
        words::action(Locale::English, words::Action::Retry),
        "Try again"
    );
    assert_eq!(words::bytes(512), "512 B");
    assert_eq!(words::bytes(1536), "1.5 KB");
    assert_eq!(words::bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    assert_eq!(words::bytes(200 * 1024), "200 KB");
    assert_eq!(words::percent(0.426), "43%");
    assert_eq!(words::percent(f32::NAN), "0%");
    assert_eq!(words::percent(2.0), "100%");
    assert_eq!(words::duration(Locale::English, 45), "45 s");
    assert_eq!(words::duration(Locale::English, 200), "3 min 20 s");
    assert_eq!(words::duration(Locale::English, 7500), "2 h 5 min");
    assert_eq!(words::duration(Locale::TraditionalChinese, 180), "3 分");
    assert_eq!(words::duration(Locale::TraditionalChinese, 3600), "1 小時");
}

#[test]
fn a_dragged_item_lands_where_it_is_dropped_and_the_rest_shift() {
    let mut items = vec!["a", "b", "c", "d"];
    assert!(move_item(&mut items, 0, 2));
    assert_eq!(items, ["b", "c", "a", "d"]);
    assert!(move_item(&mut items, 3, 0));
    assert_eq!(items, ["d", "b", "c", "a"]);
    // Dropping on itself or out of range changes nothing.
    assert!(!move_item(&mut items, 1, 1));
    assert!(!move_item(&mut items, 9, 0));
    assert!(!move_item(&mut items, 0, 4));
    assert_eq!(items, ["d", "b", "c", "a"]);
    let mut empty: Vec<u8> = Vec::new();
    assert!(!move_item(&mut empty, 0, 0));
}

/// Windows 11's Start menu draws a pinned app as a 32px icon over two lines
/// of 12px text in a 96px column; File Explorer draws a row's icon at 24px
/// beside one line and 32px beside two.
#[test]
fn tiles_and_rows_draw_application_icons_at_windows_sizes() {
    assert_eq!(TILE_ICON, px(32.0));
    assert_eq!(TILE_WIDTH, px(96.0));
    assert!(TILE_HEIGHT >= TILE_ICON + px(12.0) + px(32.0));
    assert_eq!(row_icon_size(false), px(24.0));
    assert_eq!(row_icon_size(true), px(32.0));
    assert!(row_icon_size(true) <= size::TWO_LINE_ROW_HEIGHT);
    assert!(row_icon_size(false) <= size::ROW_HEIGHT);
}

#[test]
fn a_dialog_window_has_only_a_close_button() {
    assert_eq!(caption_buttons(true), [CaptionButton::Close]);
    assert_eq!(
        caption_buttons(false),
        [
            CaptionButton::Minimize,
            CaptionButton::Maximize,
            CaptionButton::Close
        ]
    );
}

/// A dragged tile over another lights that tile's edge. The edge has to be
/// there all along, transparent, or lighting it grows the tile by 2px and
/// shifts the grid under the pointer. A GPUI style refinement cannot be
/// inspected headless, so this reads the control's source.
#[test]
fn a_tile_keeps_its_size_while_a_dragged_tile_is_over_it() {
    assert_eq!(TILE_BODY_WIDTH + px(2.0), TILE_WIDTH);
    assert_eq!(TILE_BODY_HEIGHT + px(2.0), TILE_HEIGHT);
    let source = include_str!("list.rs");
    let render = &source[source
        .find("impl RenderOnce for GridTile")
        .expect("GridTile renders")..];
    let drag_over = &render[render.find(".drag_over::<DraggedTile>").expect("drag over")..];
    let drag_over = &drag_over[..drag_over.find(".on_drop(").expect("then drop")];
    assert!(
        !drag_over.contains("border_1"),
        "the drag highlight must only colour the edge, not add one"
    );
    let before = &render[..render.find(".drag_over::<DraggedTile>").unwrap()];
    assert!(
        before.contains(".border_1()"),
        "the edge is reserved at rest"
    );
}

// Charts (`chart.rs`). WinUI has no chart; the rules are Task Manager's.

#[test]
fn a_chart_grows_in_from_the_right_and_leaves_a_gap_where_a_reading_is_missing() {
    use super::chart::runs;
    // Four readings in a chart that holds five: the first sits one step in
    // from the left and the newest at the right edge.
    let drawn = runs(&[Some(0.0), Some(50.0), None, Some(100.0)], 5, 100.0);
    assert_eq!(drawn.len(), 2, "the missing reading splits the line");
    assert_eq!(drawn[0], vec![(0.25, 0.0), (0.5, 0.5)]);
    assert_eq!(drawn[1], vec![(1.0, 1.0)]);
}

#[test]
fn a_chart_keeps_only_the_newest_readings_and_clamps_to_its_axis() {
    use super::chart::runs;
    let values: Vec<Option<f64>> = (0..10).map(|v| Some(v as f64 * 30.0)).collect();
    let drawn = runs(&values, 3, 100.0);
    assert_eq!(drawn.len(), 1);
    // 210, 240, 270 against a top of 100 are all drawn at the top.
    assert_eq!(drawn[0], vec![(0.0, 1.0), (0.5, 1.0), (1.0, 1.0)]);
    assert!(runs(&[None, None], 60, 1.0).is_empty());
    assert!(runs(&[Some(f64::NAN)], 60, 1.0).is_empty());
    // A zero or broken axis is treated as 1 rather than dividing by zero.
    assert_eq!(runs(&[Some(0.5)], 2, 0.0)[0], vec![(1.0, 0.5)]);
}

#[test]
fn a_self_scaling_axis_tops_out_at_a_round_number() {
    use super::chart::{ChartHue, ChartSeries, nice_maximum, peak};
    assert_eq!(nice_maximum(3.1, 1.0), 5.0);
    assert_eq!(nice_maximum(1_100.0, 1.0), 2_000.0);
    assert_eq!(nice_maximum(9_000.0, 1.0), 10_000.0);
    assert_eq!(nice_maximum(0.0, 1024.0), 1024.0);
    assert_eq!(nice_maximum(f64::NAN, 10.0), 10.0);
    assert_eq!(nice_maximum(100.0, 1.0), 100.0);
    let series = vec![
        ChartSeries::new(vec![Some(1.0), None, Some(7.0)], ChartHue::Green),
        ChartSeries::new(vec![Some(3.0)], ChartHue::Green),
    ];
    assert_eq!(peak(&series), Some(7.0));
    assert_eq!(peak(&[ChartSeries::new(vec![None], ChartHue::Blue)]), None);
}

#[test]
fn chart_colors_differ_per_resource_and_per_appearance() {
    use super::chart::ChartHue;
    let hues = [
        ChartHue::Blue,
        ChartHue::Purple,
        ChartHue::Green,
        ChartHue::Orange,
        ChartHue::Teal,
        ChartHue::Red,
        ChartHue::Magenta,
    ];
    for dark in [false, true] {
        for (i, a) in hues.iter().enumerate() {
            for b in &hues[i + 1..] {
                assert_ne!(a.color(dark), b.color(dark));
            }
            assert_ne!(
                a.color(dark),
                palette_for(dark).accent,
                "data is not the accent"
            );
            assert!(a.area(dark).a < 0.3, "the area is a tint");
        }
    }
    assert_ne!(ChartHue::Blue.color(false), ChartHue::Blue.color(true));
}

#[test]
fn a_composition_never_adds_up_to_more_than_the_whole() {
    use super::chart::composition;
    assert_eq!(composition(&[0.5, 0.25]), vec![0.5, 0.25]);
    assert_eq!(composition(&[0.75, 0.5, 0.1]), vec![0.75, 0.25, 0.0]);
    assert_eq!(composition(&[-1.0, f32::NAN, 2.0]), vec![0.0, 0.0, 1.0]);
}

// The details table's rich cells and keyboard (`list.rs`).

#[test]
fn a_table_cell_carries_its_tone_indent_and_expander() {
    use super::list::{CellTone, TableCell, tone_color};
    let cell = TableCell::new("Files")
        .strong()
        .indent(1)
        .expander(true)
        .tone(CellTone::Secondary);
    assert!(cell.strong);
    assert_eq!(cell.indent, 1);
    assert_eq!(cell.expander, Some(true));
    let plain: TableCell = "12%".into();
    assert_eq!(plain.tone, CellTone::Primary);
    assert_eq!(plain.expander, None);
    for dark in [false, true] {
        let p = palette_for(dark);
        assert_eq!(tone_color(CellTone::Primary, p), p.text_primary);
        assert_eq!(tone_color(CellTone::Secondary, p), p.text_secondary);
        assert_eq!(tone_color(CellTone::Caution, p), p.caution);
        assert_eq!(tone_color(CellTone::Critical, p), p.critical);
    }
}

#[test]
fn table_rows_move_with_the_arrows_and_stop_at_the_ends() {
    use super::list::table_step;
    assert_eq!(table_step(None, 3, Key::Down), Some(0));
    assert_eq!(table_step(Some(0), 3, Key::Down), Some(1));
    assert_eq!(
        table_step(Some(2), 3, Key::Down),
        None,
        "it stops at the end"
    );
    assert_eq!(table_step(Some(0), 3, Key::Up), None);
    assert_eq!(table_step(Some(1), 3, Key::End), Some(2));
    assert_eq!(table_step(Some(2), 3, Key::Home), Some(0));
    assert_eq!(table_step(Some(1), 3, Key::Enter), None);
    assert_eq!(table_step(None, 0, Key::Down), None);
}

#[test]
fn the_menu_key_opens_a_rows_menu_under_that_row() {
    use super::list::menu_anchor;
    let viewport = gpui::Bounds::new(
        gpui::point(px(300.0), px(200.0)),
        gpui::size(px(800.0), px(360.0)),
    );
    // The third row (36px each), unscrolled: under its bottom edge.
    assert_eq!(
        menu_anchor(viewport, px(0.0), 2),
        gpui::point(px(348.0), px(308.0))
    );
    // Scrolled down by two rows, the same row is two rows higher.
    assert_eq!(
        menu_anchor(viewport, px(-72.0), 2),
        gpui::point(px(348.0), px(236.0))
    );
    // A row scrolled out of view keeps the menu inside the table.
    assert_eq!(menu_anchor(viewport, px(-720.0), 2).y, px(200.0));
    assert_eq!(menu_anchor(viewport, px(0.0), 40).y, px(560.0));
}

#[test]
fn multi_line_text_box_grows_by_a_body_line_per_row() {
    assert_eq!(multi_line_height(1), gpui::px(32.0));
    assert_eq!(multi_line_height(0), gpui::px(32.0));
    assert_eq!(multi_line_height(3), gpui::px(72.0));
}

#[test]
fn a_badge_counts_what_is_waiting_and_is_absent_for_nothing() {
    assert_eq!(InfoBadge::count(0), None);
    assert_eq!(InfoBadge::count(2), Some(InfoBadge::Count(2)));
    assert_eq!(InfoBadge::Count(2).text().as_deref(), Some("2"));
    assert_eq!(
        InfoBadge::Count(MAX_SHOWN_COUNT).text().as_deref(),
        Some("99")
    );
    assert_eq!(InfoBadge::Count(100).text().as_deref(), Some("99+"));
    assert_eq!(InfoBadge::Dot.text(), None);
    let item = NavItem::new("Apps", gpui_component::IconName::Inbox).badge(InfoBadge::count(3));
    assert_eq!(item.badge, Some(InfoBadge::Count(3)));
}

/// `AutoSuggestBox`: Down and Up walk the list and past either end back to
/// the text; Enter takes the highlight or submits the text; Escape closes.
#[test]
fn the_suggestion_list_follows_the_auto_suggest_keys() {
    assert_eq!(
        suggest_key(true, None, 3, Key::Down),
        SuggestOutcome::Highlight(Some(0))
    );
    assert_eq!(
        suggest_key(true, Some(1), 3, Key::Down),
        SuggestOutcome::Highlight(Some(2))
    );
    assert_eq!(
        suggest_key(true, Some(2), 3, Key::Down),
        SuggestOutcome::Highlight(None)
    );
    assert_eq!(
        suggest_key(true, None, 3, Key::Up),
        SuggestOutcome::Highlight(Some(2))
    );
    assert_eq!(
        suggest_key(true, Some(0), 3, Key::Up),
        SuggestOutcome::Highlight(None)
    );
    assert_eq!(
        suggest_key(true, Some(1), 3, Key::Enter),
        SuggestOutcome::Choose(1)
    );
    assert_eq!(
        suggest_key(true, None, 3, Key::Enter),
        SuggestOutcome::Submit
    );
    // A highlight left over from a closed list, or past a list that shrank,
    // is not a choice.
    assert_eq!(
        suggest_key(false, Some(1), 3, Key::Enter),
        SuggestOutcome::Submit
    );
    assert_eq!(
        suggest_key(true, Some(5), 3, Key::Enter),
        SuggestOutcome::Submit
    );
    assert_eq!(
        suggest_key(true, None, 3, Key::Escape),
        SuggestOutcome::Close
    );
    assert_eq!(
        suggest_key(false, None, 3, Key::Escape),
        SuggestOutcome::Ignored
    );
    assert_eq!(
        suggest_key(false, None, 3, Key::Down),
        SuggestOutcome::Ignored
    );
    assert_eq!(
        suggest_key(true, None, 0, Key::Down),
        SuggestOutcome::Ignored
    );
    assert_eq!(
        suggest_key(true, None, 3, Key::Left),
        SuggestOutcome::Ignored
    );
}

#[test]
fn the_suggestion_list_opens_for_text_and_stays_shut_after_escape() {
    assert!(list_open(true, "cha", None));
    assert!(!list_open(false, "cha", None));
    assert!(!list_open(true, "", None));
    assert!(!list_open(true, "   ", None));
    assert!(!list_open(true, "cha", Some("cha")));
    assert!(list_open(true, "char", Some("cha")));
}

#[test]
fn the_pane_search_is_a_field_open_and_a_button_compact() {
    assert_eq!(search_slot(false, false), SearchSlot::None);
    assert_eq!(search_slot(false, true), SearchSlot::None);
    assert_eq!(search_slot(true, false), SearchSlot::Field);
    assert_eq!(search_slot(true, true), SearchSlot::Button);
}

/// A narrow window's open pane floats: the content stays 48px in, and the
/// pane is never gone.
#[test]
fn an_open_pane_in_a_narrow_window_floats_over_the_content() {
    assert!(floats_over_content(false, true));
    assert!(!floats_over_content(true, true));
    assert!(!floats_over_content(false, false));
    assert_eq!(content_offset(false, true), px(48.0));
    assert_eq!(content_offset(true, true), px(48.0));
    assert_eq!(content_offset(false, false), px(320.0));
    assert_eq!(content_offset(true, false), px(48.0));
}

#[test]
fn back_is_alt_left_and_says_so_in_both_languages() {
    assert!(is_back_shortcut("left", false, true, false, false));
    assert!(!is_back_shortcut("left", false, false, false, false));
    assert!(!is_back_shortcut("left", true, true, false, false));
    assert!(!is_back_shortcut("left", false, true, true, false));
    assert!(!is_back_shortcut("right", false, true, false, false));
    assert_eq!(back_tooltip(Locale::English), "Back");
    assert_eq!(back_tooltip(Locale::TraditionalChinese), "返回");
}

#[test]
fn a_link_is_accent_text_and_disabled_text_when_disabled() {
    for p in [&LIGHT, &DARK] {
        assert_eq!(link_color(Interaction::Rest, p), p.accent_text);
        assert_eq!(link_color(Interaction::PointerOver, p), p.accent_text);
        assert_eq!(link_color(Interaction::Disabled, p), p.text_disabled);
        assert!(link_color(Interaction::Pressed, p).a < 1.0);
    }
}

/// A status glyph in a list row takes the color an info bar gives the same
/// severity's icon, never a fill.
#[test]
fn a_status_glyph_is_the_severity_color() {
    use super::list::status_glyph_color;
    for p in [&LIGHT, &DARK] {
        assert_eq!(status_glyph_color(Severity::Success, p), p.success);
        assert_eq!(status_glyph_color(Severity::Warning, p), p.caution);
        assert_eq!(status_glyph_color(Severity::Error, p), p.critical);
        assert_eq!(status_glyph_color(Severity::Informational, p), p.accent);
    }
}

// The branch graph's cell (`lanes.rs`) and the label chip (`chip.rs`).

#[test]
fn lane_lines_run_straight_in_their_column_and_curve_between_columns() {
    use super::lanes::{LaneLine, LineKind, column_x, segment};
    let line = |kind, column| LaneLine {
        kind,
        column,
        lane: 0,
    };
    assert_eq!(column_x(0, 16.0), 8.0);
    assert_eq!(column_x(2, 16.0), 40.0);
    // Pass: top to bottom, straight.
    let pass = segment(line(LineKind::Pass, 1), 0, 16.0, 48.0);
    assert_eq!(
        (pass.from, pass.to, pass.controls),
        ((24.0, 0.0), (24.0, 48.0), None)
    );
    // In from the node's own column: straight to the node's centre.
    let straight = segment(line(LineKind::In, 0), 0, 16.0, 48.0);
    assert_eq!((straight.from, straight.to), ((8.0, 0.0), (8.0, 24.0)));
    assert!(straight.controls.is_none());
    // A fork from column 2 into a node in column 0 curves and leaves and
    // arrives vertically: both controls at mid-height, over each end.
    let fork = segment(line(LineKind::In, 2), 0, 16.0, 48.0);
    assert_eq!(fork.from, (40.0, 0.0));
    assert_eq!(fork.to, (8.0, 24.0));
    assert_eq!(fork.controls, Some(((40.0, 12.0), (8.0, 12.0))));
    // A merge line from the node to column 1 at the bottom.
    let merge = segment(line(LineKind::Out, 1), 0, 16.0, 48.0);
    assert_eq!(merge.from, (8.0, 24.0));
    assert_eq!(merge.to, (24.0, 48.0));
    assert_eq!(merge.controls, Some(((8.0, 36.0), (24.0, 36.0))));
}

#[test]
fn a_lane_keeps_one_hue_and_neighbouring_lanes_differ() {
    use super::chart::ChartHue;
    use super::lanes::lane_hue;
    assert_eq!(lane_hue(0), ChartHue::SEQUENCE[0]);
    for lane in 0..20u32 {
        assert_eq!(lane_hue(lane), lane_hue(lane));
        assert_ne!(lane_hue(lane), lane_hue(lane + 1));
        assert_eq!(
            lane_hue(lane),
            lane_hue(lane + ChartHue::SEQUENCE.len() as u32)
        );
    }
    let hues: std::collections::HashSet<ChartHue> = ChartHue::SEQUENCE.into_iter().collect();
    assert_eq!(hues.len(), 5, "five different hues");
    assert!(
        !hues.contains(&ChartHue::Red) && !hues.contains(&ChartHue::Magenta),
        "the alarm red and the NPU magenta are not lane colours"
    );
}

#[test]
fn a_chip_takes_its_hue_or_the_control_colours_and_primary_text() {
    use super::chart::ChartHue;
    use super::chip::chip_colors;
    for p in [&LIGHT, &DARK] {
        let dark = p.is_dark();
        assert_eq!(
            chip_colors(Some(ChartHue::Green), p),
            (
                ChartHue::Green.area(dark),
                ChartHue::Green.color(dark),
                p.text_primary
            )
        );
        assert_eq!(
            chip_colors(None, p),
            (p.control_fill, p.stroke, p.text_primary)
        );
        assert_ne!(chip_colors(None, p).0, p.accent, "a chip is never accent");
    }
}

#[test]
fn a_clipped_cell_marks_what_is_beyond_and_never_hides_its_node() {
    use super::lanes::{LaneLine, LineKind, clipping};
    let line = |column| LaneLine {
        kind: LineKind::Pass,
        column,
        lane: 0,
    };
    let inside = clipping(1, &[line(0), line(2)], 3);
    assert_eq!(
        (inside.node_column, inside.node_clipped, inside.beyond),
        (1, false, false)
    );
    let lines_beyond = clipping(1, &[line(0), line(5)], 3);
    assert_eq!(
        (
            lines_beyond.node_column,
            lines_beyond.node_clipped,
            lines_beyond.beyond
        ),
        (1, false, true)
    );
    let node_beyond = clipping(7, &[line(0)], 3);
    assert_eq!(
        (
            node_beyond.node_column,
            node_beyond.node_clipped,
            node_beyond.beyond
        ),
        (3, true, true),
        "drawn in the overflow column, on no lane of its own"
    );
    assert_eq!(
        clipping(4, &[], 0).node_column,
        1,
        "at least one lane shows, then the overflow column"
    );
}

// --- light dismiss and hit areas (ticket 103) --------------------------------

use super::hit_area::{self as hit, RowInset};
use super::light_dismiss::{Opener, PressAt, press, three_presses};

/// Every kit control with a light-dismiss panel: open with a press, close
/// with a second press on the same control, open again with a third.
#[test]
fn every_light_dismiss_panel_closes_on_a_second_press_of_its_control() {
    let controls = [
        ("Flyout", Opener::Shielded),
        ("ComboBox", Opener::Shielded),
        ("MenuFlyout", Opener::Shielded),
        ("SplitButton", Opener::Shielded),
        ("DropDownButton", Opener::Shielded),
        ("CommandBar More", Opener::Shielded),
        ("BreadcrumbBar overflow", Opener::Shielded),
        ("NavigationView floating pane", Opener::InsidePanel),
        ("AutoSuggestBox", Opener::Field),
    ];
    for (name, opener) in controls {
        assert_eq!(three_presses(opener), [true, false, true], "{name}");
    }
}

#[test]
fn a_press_outside_closes_the_panel_and_still_reaches_its_target() {
    assert_eq!(
        press(true, PressAt::Elsewhere),
        super::light_dismiss::Press {
            open: false,
            reaches_target: true
        }
    );
    // The closing press on the panel's own control is taken by the shield.
    assert!(!press(true, PressAt::OwnControl).reaches_target);
    assert!(press(true, PressAt::Panel).open);
    // A press in a menu opened from the panel, past the panel's edge.
    assert!(press(true, PressAt::ChildPanel).open);
    assert!(press(true, PressAt::ChildPanel).reaches_target);
    assert!(!press(false, PressAt::Elsewhere).open);
}

/// Every panel the kit closes on `on_mouse_down_out` either floats through
/// `anchored_panel`, which shields its control, or is the one panel whose
/// toggle is inside it.
#[test]
fn every_panel_that_closes_on_a_press_outside_shields_its_control() {
    let sources = [
        ("dialog.rs", include_str!("dialog.rs")),
        ("menu.rs", include_str!("menu.rs")),
        ("selection.rs", include_str!("selection.rs")),
        ("navigation.rs", include_str!("navigation.rs")),
    ];
    for (name, source) in sources {
        assert!(source.contains(".on_mouse_down_out("), "{name}");
        let shielded = source.contains("anchored_panel(");
        assert!(
            shielded || name == "navigation.rs",
            "{name} closes on a press outside without shielding its control"
        );
    }
    // Every panel that closes on a press outside first asks whether the
    // press was in a panel opened from it.
    for (name, source) in sources {
        assert!(source.contains("pointer_in_child_panel("), "{name}");
        assert!(source.contains("panel_marker("), "{name}");
    }
    // The menu's context form has no control to shield; its flyout form does.
    assert!(include_str!("menu.rs").contains("light_dismiss::anchored_panel(panel, self.edge)"));
}

#[test]
fn a_press_in_the_suggestion_field_closes_the_list_and_the_next_opens_it() {
    use super::auto_suggest::field_pressed;
    let value = "git";
    // Open: the press closes it for this text.
    let dismissed = field_pressed(true, value, None);
    assert_eq!(dismissed.as_deref(), Some(value));
    assert!(!list_open(true, value, dismissed.as_deref()));
    // Closed by that press: the next one opens it again.
    let dismissed = field_pressed(false, value, dismissed.as_deref());
    assert_eq!(dismissed, None);
    assert!(list_open(true, value, dismissed.as_deref()));
    // Closed for another text: a press keeps what it remembered.
    assert_eq!(
        field_pressed(false, value, Some("gi")).as_deref(),
        Some("gi")
    );
}

#[test]
fn a_menu_items_highlight_goes_when_the_pointer_leaves_it() {
    let mut menu = MenuOpen::detached();
    menu.toggle();
    menu.pointer_over(2);
    assert_eq!(menu.highlight, Some(2));
    menu.pointer_left(2);
    assert_eq!(menu.highlight, None, "a row left behind looks clickable");
    // The keyboard moved on while the pointer was still on row 2.
    menu.pointer_over(2);
    menu.highlight = Some(3);
    menu.pointer_left(2);
    assert_eq!(menu.highlight, Some(3));
    // An open submenu keeps its row highlighted while the pointer goes to it.
    menu.highlight = Some(4);
    menu.submenu = Some(4);
    menu.pointer_left(4);
    assert_eq!(menu.highlight, Some(4));
    menu.pointer_over_sub(1);
    menu.pointer_left_sub(1);
    assert_eq!(menu.sub_highlight, None);
}

/// The kit's rows, each with its inset, drawn height, and where its label
/// ends and its trailing text starts, pressed at scale 1 and 2.
#[test]
fn every_kit_row_takes_a_click_anywhere_it_is_highlighted() {
    /// A row's name, inset, drawn height, and label-to-trailing gap.
    type KitRow = (&'static str, RowInset, f32, Option<(f32, f32)>);
    let rows: [KitRow; 8] = [
        // Label, then the shortcut 24px after it.
        ("menu item", RowInset::MENU, 36.0, Some((90.0, 150.0))),
        ("combo item", RowInset::COMBO, 32.0, None),
        ("suggestion", RowInset::SUGGESTION, 32.0, None),
        // Label, then a badge at the trailing edge.
        (
            "navigation item",
            RowInset::NAVIGATION,
            36.0,
            Some((120.0, 250.0)),
        ),
        ("list row", RowInset::LIST, 36.0, Some((100.0, 200.0))),
        ("two-line list row", RowInset::LIST, 48.0, None),
        // Rows that fill their row already: no inset.
        (
            "table row",
            RowInset { x: 0.0, y: 0.0 },
            36.0,
            Some((80.0, 200.0)),
        ),
        (
            "settings card",
            RowInset { x: 0.0, y: 0.0 },
            68.0,
            Some((150.0, 250.0)),
        ),
    ];
    for (name, inset, height, gap) in rows {
        let stack = hit::stack(inset, &[height, height, height], 300.0);
        for scale in [1.0, 2.0] {
            if let Err(problem) = hit::check_rows(&stack, gap, scale) {
                panic!("{name}: {problem}");
            }
        }
    }
}

/// The old way, margins outside the drawn item, fails the same test: the
/// pixels between two items belong to no row.
#[test]
fn rows_drawn_with_margins_leave_dead_pixels_between_them() {
    let inset = RowInset::MENU;
    let looks: Vec<hit::RowGeometry> = hit::stack(inset, &[36.0, 36.0], 300.0)
        .into_iter()
        .map(|row| hit::RowGeometry {
            hit: row.look,
            look: row.look,
        })
        .collect();
    for scale in [1.0, 2.0] {
        assert!(hit::check_rows(&looks, None, scale).is_err());
    }
}

/// Each row renderer builds its hit area from the inset the test reads.
#[test]
fn the_row_renderers_use_their_hit_area_inset() {
    for (name, source, inset) in [
        ("menu.rs", include_str!("menu.rs"), "RowInset::MENU"),
        (
            "selection.rs",
            include_str!("selection.rs"),
            "RowInset::COMBO",
        ),
        (
            "auto_suggest.rs",
            include_str!("auto_suggest.rs"),
            "RowInset::SUGGESTION",
        ),
        (
            "navigation.rs",
            include_str!("navigation.rs"),
            "RowInset::NAVIGATION",
        ),
        ("list.rs", include_str!("list.rs"), "RowInset::LIST"),
    ] {
        assert!(source.contains("hit_row("), "{name}");
        assert!(source.contains(inset), "{name}");
        // A margin of 2px above and below was the gap between rows.
        assert!(
            !source.contains(".my(px(2.0))"),
            "{name} draws a row with margins again"
        );
    }
}

/// A window with nothing in it, to apply an appearance to.
struct Blank;

impl gpui::Render for Blank {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div()
    }
}

thread_local! {
    static PAINTED: std::cell::RefCell<Vec<bool>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn record_paint(dark: bool, _: &mut gpui::Window, _: &mut gpui::App) {
    PAINTED.with(|painted| painted.borrow_mut().push(dark));
}

/// Ticket 110: a window opens before GPUI has heard the desktop's appearance
/// from the portal, so GPUI reports light at first (its test window does the
/// same). A stored `system` has to paint now and keep a subscription for the
/// real answer; an explicit choice paints once and needs none.
#[gpui::test]
fn following_the_system_paints_now_and_keeps_listening(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let (_, cx) = cx.add_window_view(|_, _| Blank);
    cx.update(|window, cx| {
        assert_eq!(window.appearance(), gpui::WindowAppearance::Light);

        PAINTED.with(|painted| painted.borrow_mut().clear());
        let follow = Appearance::System.follow(window, cx, record_paint);
        assert!(follow.is_some(), "system must keep following the desktop");
        let dark = Appearance::Dark.follow(window, cx, record_paint);
        assert!(dark.is_none());
        let light = Appearance::Light.follow(window, cx, record_paint);
        assert!(light.is_none());
        assert_eq!(
            PAINTED.with(|painted| painted.borrow().clone()),
            [false, true, false]
        );

        // The Fluent windows' own call does the same and repaints the kit.
        let follow = Appearance::System.apply_and_follow(window, cx);
        assert!(follow.is_some());
        assert!(!gpui_component::Theme::global(cx).mode.is_dark());
        assert!(Appearance::Dark.apply_and_follow(window, cx).is_none());
        assert!(gpui_component::Theme::global(cx).mode.is_dark());
    });
}

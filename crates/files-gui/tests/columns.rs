//! The list's columns step aside, extension first, so the name stays readable.

use files_gui::content::{ListColumn, MIN_NAME_WIDTH, visible_columns};

#[test]
fn a_wide_list_shows_every_column() {
    assert_eq!(visible_columns(2_000.0), ListColumn::ALL.to_vec());
}

#[test]
fn a_narrow_list_drops_extension_then_type_and_always_keeps_the_name() {
    let mid = visible_columns(700.0);
    assert!(!mid.contains(&ListColumn::Extension));
    assert!(mid.contains(&ListColumn::Name));
    let narrow = visible_columns(300.0);
    assert_eq!(narrow, vec![ListColumn::Name]);
    for width in [300.0, 500.0, 700.0, 900.0, 1_200.0] {
        let shown = visible_columns(width);
        let others: f32 = shown
            .iter()
            .filter(|column| **column != ListColumn::Name)
            .map(|column| column.width() + 8.0)
            .sum();
        assert!(
            shown.len() == 1 || width - others >= MIN_NAME_WIDTH,
            "the name keeps its room at {width}"
        );
    }
}

#[test]
fn a_tile_says_a_files_size_and_day_and_a_folders_day() {
    use files_core::{Entry, EntryKind, EntrySize, LocalPath};
    use files_gui::content::{rendered_row, tile_caption};
    let mut file = Entry::file(
        "a.txt",
        LocalPath::new("/d/a.txt").unwrap(),
        EntryKind::File,
    );
    file.size = EntrySize::Bytes(2048);
    file.modified = Some(files_core::FileTime::new(1_762_000_000, 0));
    let row = rendered_row(&file, false, false, &files_gui::i18n::EN_US);
    let caption = tile_caption(&row);
    assert!(caption.contains(" · 2025-11-01"), "{caption}");
    let mut folder = Entry::file("d", LocalPath::new("/d/d").unwrap(), EntryKind::Directory);
    folder.modified = file.modified;
    assert_eq!(
        tile_caption(&rendered_row(
            &folder,
            false,
            false,
            &files_gui::i18n::EN_US
        )),
        "2025-11-01"
    );
    let bare = Entry::file("x", LocalPath::new("/d/x").unwrap(), EntryKind::File);
    assert_eq!(
        tile_caption(&rendered_row(&bare, false, false, &files_gui::i18n::EN_US)),
        ""
    );
}

#[test]
fn ctrl_wheel_up_is_larger_down_is_smaller_and_stops_at_the_ends() {
    use files_gui::prefs::{ItemScale, zoom_step};
    assert_eq!(zoom_step(ItemScale::Medium, 1.0), ItemScale::Large);
    assert_eq!(zoom_step(ItemScale::Medium, -3.0), ItemScale::Small);
    assert_eq!(zoom_step(ItemScale::Large, 1.0), ItemScale::Large);
    assert_eq!(zoom_step(ItemScale::Small, -1.0), ItemScale::Small);
    assert_eq!(zoom_step(ItemScale::Small, 0.0), ItemScale::Small);
}

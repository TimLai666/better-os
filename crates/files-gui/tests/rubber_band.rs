//! Rubber-band selection: which tiles or rows a dragged rectangle touches.

use files_gui::content::{BandLayout, band_hits};

const GRID: BandLayout = BandLayout::Grid {
    columns: 4,
    tile: 100.0,
    gap: 10.0,
    pad: 16.0,
};

#[test]
fn a_band_over_two_tiles_of_the_first_row_selects_those_two() {
    // Tiles of row 0 span y 10..110; columns start at x 16, 126, 236, 346.
    assert_eq!(band_hits((20.0, 20.0), (130.0, 50.0), GRID, 10), [0, 1]);
}

#[test]
fn a_band_drawn_upwards_and_leftwards_is_the_same_band() {
    assert_eq!(
        band_hits((130.0, 50.0), (20.0, 20.0), GRID, 10),
        band_hits((20.0, 20.0), (130.0, 50.0), GRID, 10)
    );
}

#[test]
fn a_band_spanning_rows_takes_the_block_and_stops_at_the_last_item() {
    // Rows 0..=2, columns 2..=3; item 11 does not exist.
    assert_eq!(
        band_hits((240.0, 15.0), (400.0, 240.0), GRID, 10),
        [2, 3, 6, 7]
    );
    assert_eq!(band_hits((0.0, 0.0), (1000.0, 1000.0), GRID, 10).len(), 10);
}

#[test]
fn a_band_in_the_gap_between_tiles_touches_nothing() {
    assert!(band_hits((117.0, 20.0), (125.0, 60.0), GRID, 10).is_empty());
}

#[test]
fn the_list_takes_every_row_the_band_crosses() {
    let list = BandLayout::List { row: 36.0 };
    assert_eq!(band_hits((5.0, 40.0), (5.0, 110.0), list, 10), [1, 2, 3]);
    assert_eq!(band_hits((5.0, 340.0), (5.0, 900.0), list, 10), [9]);
}

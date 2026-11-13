//! The sidebar draws only places that exist; a missing pin stays.

use files_core::Location;
use files_gui::sidebar::{Availability, SidebarRow, SidebarSection, shown_rows};

fn row(section: SidebarSection, key: &str, available: bool) -> SidebarRow {
    SidebarRow {
        section,
        key: key.to_string(),
        label: key.to_string(),
        location: Location::local(format!("/x/{key}")).unwrap(),
        availability: if available {
            Availability::Available
        } else {
            Availability::Unavailable
        },
        bookmark_index: (section == SidebarSection::Favorites).then_some(0),
        device_state: None,
        identity_volatile: false,
    }
}

#[test]
fn a_missing_place_is_left_out_and_a_missing_pin_stays() {
    let rows = vec![
        row(SidebarSection::Places, "documents", true),
        row(SidebarSection::Places, "music", false),
        row(SidebarSection::Favorites, "gone", false),
    ];
    let keys: Vec<String> = shown_rows(rows).into_iter().map(|row| row.key).collect();
    assert_eq!(keys, ["documents", "gone"]);
}

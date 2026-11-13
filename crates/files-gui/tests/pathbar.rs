//! The address bar's crumbs: Home first under home, `/` first elsewhere.

use std::path::Path;

use files_core::{Location, TrashLocation};
use files_gui::i18n::{EN_US, ZH_TW};
use files_gui::pathbar::{crumbs, visible_crumbs};

fn labels(location: &Location, home: Option<&Path>) -> Vec<String> {
    crumbs(location, home, &EN_US)
        .into_iter()
        .map(|crumb| crumb.label)
        .collect()
}

#[test]
fn a_folder_under_home_starts_from_home() {
    let location = Location::local("/home/ann/Documents/reports").unwrap();
    assert_eq!(
        labels(&location, Some(Path::new("/home/ann"))),
        ["Home", "Documents", "reports"]
    );
    let zh: Vec<String> = crumbs(&location, Some(Path::new("/home/ann")), &ZH_TW)
        .into_iter()
        .map(|crumb| crumb.label)
        .collect();
    assert_eq!(zh[0], ZH_TW.place_home);
}

#[test]
fn each_crumb_opens_its_own_folder() {
    let location = Location::local("/home/ann/Documents/reports").unwrap();
    let all = crumbs(&location, Some(Path::new("/home/ann")), &EN_US);
    assert_eq!(all[0].location, Location::local("/home/ann").unwrap());
    assert_eq!(
        all[1].location,
        Location::local("/home/ann/Documents").unwrap()
    );
    assert_eq!(all[2].location, location);
}

#[test]
fn a_folder_outside_home_starts_from_the_root() {
    let location = Location::local("/usr/share").unwrap();
    assert_eq!(
        labels(&location, Some(Path::new("/home/ann"))),
        ["/", "usr", "share"]
    );
    assert_eq!(labels(&Location::local("/").unwrap(), None), ["/"]);
    // A folder whose name only starts like home is not under it.
    assert_eq!(
        labels(
            &Location::local("/home/anna").unwrap(),
            Some(Path::new("/home/ann"))
        ),
        ["/", "home", "anna"]
    );
}

#[test]
fn trash_and_applications_are_one_crumb() {
    assert_eq!(
        labels(&Location::Trash(TrashLocation::Root), None),
        ["Trash"]
    );
    assert_eq!(labels(&Location::Applications, None), ["Applications"]);
}

#[test]
fn a_narrow_bar_folds_the_oldest_crumbs_and_keeps_the_current_one() {
    let location = Location::local("/home/ann/a/very/deep/folder/structure/here").unwrap();
    let all = crumbs(&location, Some(Path::new("/home/ann")), &EN_US);
    assert_eq!(visible_crumbs(&all, 10_000.0), all.len());
    let narrow = visible_crumbs(&all, 200.0);
    assert!(narrow < all.len());
    assert!(narrow >= 2, "the fold and at least the current folder");
    assert_eq!(visible_crumbs(&all, 1.0), 2);
}

#[test]
fn two_crumbs_in_a_narrow_bar_fold_to_the_current_folder() {
    // Half of a split window: "Home › Documents" does not fit, so the bar
    // shows "… › Documents" rather than "Home" with Documents clipped off.
    let location = Location::local("/home/ann/Documents").unwrap();
    let all = crumbs(&location, Some(Path::new("/home/ann")), &EN_US);
    assert_eq!(all.len(), 2);
    let visible = visible_crumbs(&all, 96.0);
    let (hidden, shown) = better_ui::fluent::breadcrumb::fold(all.len(), visible);
    assert_eq!(hidden, vec![0]);
    assert_eq!(shown, vec![1]);
    // Wide enough, both show.
    let visible = visible_crumbs(&all, 400.0);
    assert_eq!(
        better_ui::fluent::breadcrumb::fold(all.len(), visible).0,
        Vec::<usize>::new()
    );
}

mod completion {
    use std::fs;
    use std::path::Path;

    use files_gui::pathbar::complete_path;

    fn tree() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for folder in ["Documents", "Downloads", "Desktop", "Music", ".config"] {
            fs::create_dir(root.path().join(folder)).unwrap();
        }
        fs::write(root.path().join("Doc.txt"), b"a file is not completed").unwrap();
        root
    }

    #[test]
    fn one_match_completes_with_a_slash() {
        let root = tree();
        let base = root.path().display().to_string();
        assert_eq!(
            complete_path(&format!("{base}/Mu"), None),
            Some(format!("{base}/Music/"))
        );
        assert_eq!(
            complete_path(&format!("{base}/mu"), None),
            Some(format!("{base}/Music/")),
            "case only matters when it can"
        );
    }

    #[test]
    fn several_complete_as_far_as_they_agree() {
        let root = tree();
        let base = root.path().display().to_string();
        assert_eq!(
            complete_path(&format!("{base}/Do"), None),
            None,
            "Documents and Downloads agree on nothing past Do"
        );
        assert_eq!(
            complete_path(&format!("{base}/D"), None),
            None,
            "Desktop, Documents, Downloads agree only on D"
        );
        assert_eq!(
            complete_path(&format!("{base}/Doc"), None),
            Some(format!("{base}/Documents/"))
        );
    }

    #[test]
    fn home_hidden_folders_and_nonsense() {
        let root = tree();
        assert_eq!(
            complete_path("~/Mus", Some(root.path())),
            Some(format!("{}/Music/", root.path().display()))
        );
        let base = root.path().display().to_string();
        assert_eq!(
            complete_path(&format!("{base}/.co"), None),
            Some(format!("{base}/.config/"))
        );
        assert_eq!(complete_path(&format!("{base}/Zzz"), None), None);
        assert_eq!(complete_path("relative/path", None), None);
        assert_eq!(
            complete_path("/no/such/folder/x", Some(Path::new("/h"))),
            None
        );
    }
}

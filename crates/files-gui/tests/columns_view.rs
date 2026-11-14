//! The columns view: which folders stand to the left, and what each holds.

use std::fs;
use std::path::Path;

use files_gui::columns::{ancestor_columns, column_entries};

#[test]
fn the_folders_above_are_shown_from_home_or_from_the_root_at_most_three() {
    let home = Path::new("/home/ann");
    assert_eq!(
        ancestor_columns(Path::new("/home/ann/Documents/work"), Some(home)),
        [Path::new("/home/ann"), Path::new("/home/ann/Documents")]
    );
    assert!(ancestor_columns(home, Some(home)).len() <= 3);
    assert_eq!(
        ancestor_columns(Path::new("/a/b/c/d/e"), Some(home)),
        [
            Path::new("/a/b"),
            Path::new("/a/b/c"),
            Path::new("/a/b/c/d")
        ]
    );
    assert!(ancestor_columns(Path::new("/"), None).is_empty());
}

#[test]
fn a_column_lists_folders_first_by_name_and_hides_dotfiles() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("zeta")).unwrap();
    fs::create_dir(root.path().join("Alpha")).unwrap();
    fs::write(root.path().join("b.txt"), b"").unwrap();
    fs::write(root.path().join(".hidden"), b"").unwrap();
    let names: Vec<String> = column_entries(root.path(), false)
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(names, ["Alpha", "zeta", "b.txt"]);
    assert_eq!(column_entries(root.path(), true).len(), 4);
    assert!(column_entries(&root.path().join("missing"), false).is_empty());
}

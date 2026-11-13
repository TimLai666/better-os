//! "Show in folder" from other applications.

use files_core::LocalPath;
use files_gui::filemanager1::{local_path, targets};
use files_gui::launch::StartTarget;

#[test]
fn show_items_opens_the_folder_with_the_item_selected() {
    let got = targets(&["file:///home/a/My%20Docs/report.pdf".to_string()], true);
    assert_eq!(
        got,
        [StartTarget::File {
            folder: LocalPath::new("/home/a/My Docs").unwrap(),
            name: "report.pdf".into(),
        }]
    );
}

#[test]
fn show_folders_opens_each_folder_and_ignores_what_is_not_local() {
    let got = targets(
        &[
            "file:///srv/a".to_string(),
            "sftp://host/x".to_string(),
            "file://otherhost/x".to_string(),
            "file:///bad%zz".to_string(),
            "file://localhost/srv/b".to_string(),
        ],
        false,
    );
    assert_eq!(
        got,
        [
            StartTarget::Folder(LocalPath::new("/srv/a").unwrap()),
            StartTarget::Folder(LocalPath::new("/srv/b").unwrap()),
        ]
    );
    assert_eq!(
        local_path("file:///%E6%96%87%E4%BB%B6").unwrap().to_str(),
        Some("/文件")
    );
}

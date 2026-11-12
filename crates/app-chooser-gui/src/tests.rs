//! The chooser's plain model and wording, tested with no display backend:
//! what the list shows with and without a search, where the keyboard goes,
//! which app is selected on open, why an app is offered and how that is said
//! in both languages, and how tall the dialog is.

use std::path::PathBuf;

use app_catalog_core::{
    ApplicationRecord, DesktopEnvironments, DesktopFile, DesktopId, EntryScope, MimeType,
    NoCanonicalExecutable, NoProbe,
};
use app_chooser_core::{
    ChooserRequest, ChooserSections, ExecutableResolution, ExecutableWarning, MimeAppsFile,
    MimeGraph, UsageHistory, rank,
};
use better_ui::fluent::Key;

use crate::chooser::{executable_message, warning_message};
use crate::i18n::{Locale, always_use, copy, other_apps, reason_text, select_title};
use crate::{
    FileKind, Line, MAX_DIALOG_HEIGHT, MIN_WINDOW_HEIGHT, Reason, dialog_height, extension,
    file_kind, initial_selection, lines, list_height_limit, metrics, mime_comment, name_matches,
    reason, step, visible_selection,
};

const LOCALES: [Locale; 2] = [Locale::EnUs, Locale::ZhTw];

fn record(desktop_id: &str, name: &str, mime: &str) -> ApplicationRecord {
    let body =
        format!("[Desktop Entry]\nType=Application\nName={name}\nExec=run %U\nMimeType={mime}\n");
    let file = DesktopFile::parse(&body).expect("valid entry");
    ApplicationRecord::from_desktop_file(
        DesktopId::new(desktop_id).expect("valid id"),
        PathBuf::from(format!("/usr/share/applications/{desktop_id}")),
        EntryScope::System,
        &file,
        &NoProbe,
    )
    .expect("valid record")
}

/// Two apps that open text, one that opens images, and the default set in
/// the person's own associations.
fn sections(default: Option<&str>) -> ChooserSections {
    let records = vec![
        record("writer.desktop", "Writer", "text/plain;"),
        record("editor.desktop", "Text Editor", "text/plain;"),
        record("viewer.desktop", "Image Viewer", "image/png;"),
        record("player.desktop", "Music Player", "audio/mpeg;"),
    ];
    let mut text = String::from("[Default Applications]\n");
    if let Some(default) = default {
        text.push_str(&format!("text/plain={default}\n"));
    }
    let associations = MimeAppsFile::parse(&text).associations();
    let mime = MimeType::parse("text/plain").unwrap();
    let resolution = MimeGraph::from_data_dirs(Vec::<PathBuf>::new()).resolve(&mime);
    rank(
        &records,
        &ChooserRequest {
            resolution: &resolution,
            associations: &associations,
            history: &UsageHistory::new(),
            environments: &DesktopEnvironments::default(),
            locale: None,
        },
    )
}

fn names(lines: &[Line<'_>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| match line {
            Line::App(app) => app.display_name.clone(),
            Line::OtherApps { count, expanded } => format!("Other {count} {expanded}"),
        })
        .collect()
}

#[test]
fn the_other_apps_are_folded_away_until_asked_for() {
    let sections = sections(None);
    assert_eq!(
        names(&lines(&sections, "", false)),
        ["Text Editor", "Writer", "Other 2 false"]
    );
    assert_eq!(
        names(&lines(&sections, "", true)),
        [
            "Text Editor",
            "Writer",
            "Other 2 true",
            "Image Viewer",
            "Music Player"
        ]
    );
}

#[test]
fn a_search_looks_through_every_app_without_folding() {
    let sections = sections(None);
    assert_eq!(names(&lines(&sections, "pla", false)), ["Music Player"]);
    assert_eq!(
        names(&lines(&sections, "  ", false)),
        names(&lines(&sections, "", false))
    );
    assert!(lines(&sections, "zzz", false).is_empty());
    assert!(name_matches("Text Editor", "edi"));
    assert!(name_matches("文字編輯器", "編輯"));
    assert!(
        !name_matches("Writer", "rit"),
        "a word has to begin with it"
    );
}

#[test]
fn the_dialog_opens_with_the_default_or_the_first_recommended_app_selected() {
    let with_default = sections(Some("writer.desktop"));
    assert_eq!(
        initial_selection(&with_default).unwrap().as_str(),
        "writer.desktop"
    );
    assert_eq!(
        initial_selection(&sections(None)).unwrap().as_str(),
        "editor.desktop"
    );
    assert_eq!(initial_selection(&ChooserSections::default()), None);
}

#[test]
fn the_keyboard_walks_the_list_and_stops_at_its_ends() {
    let sections = sections(None);
    let visible = lines(&sections, "", false);
    assert_eq!(step(&visible, Some(0), Key::Down), Some(1));
    assert_eq!(
        step(&visible, Some(1), Key::Down),
        Some(2),
        "onto Other apps"
    );
    assert_eq!(step(&visible, Some(2), Key::Down), None);
    assert_eq!(step(&visible, Some(0), Key::Up), None);
    assert_eq!(step(&visible, None, Key::Down), Some(0));
    assert_eq!(step(&visible, Some(1), Key::Home), Some(0));
}

#[test]
fn every_reason_is_a_plain_sentence_in_both_languages() {
    let sections = sections(Some("writer.desktop"));
    let writer = sections
        .find(&DesktopId::new("writer.desktop").unwrap())
        .unwrap();
    assert_eq!(reason(writer), Reason::CurrentDefault);
    let editor = sections
        .find(&DesktopId::new("editor.desktop").unwrap())
        .unwrap();
    assert_eq!(reason(editor), Reason::Declares);
    let viewer = sections
        .find(&DesktopId::new("viewer.desktop").unwrap())
        .unwrap();
    assert_eq!(reason(viewer), Reason::NotDeclared);

    let reasons = [
        Reason::CurrentDefault,
        Reason::Declares,
        Reason::Related,
        Reason::Wildcard,
        Reason::UserAssociated,
        Reason::PreviouslyUsed,
        Reason::NotDeclared,
    ];
    for locale in LOCALES {
        let mut seen = Vec::new();
        for reason in reasons {
            let text = reason_text(locale, reason);
            assert!(!text.trim().is_empty());
            for jargon in [
                "MIME", "mime", "/", ".desktop", "declare", "宣告", "Flatpak",
            ] {
                assert!(!text.contains(jargon), "{locale:?} {reason:?}: {text}");
            }
            assert!(!seen.contains(&text), "two reasons say the same thing");
            seen.push(text);
        }
    }
}

#[test]
fn the_title_and_the_check_box_name_the_file_in_both_languages() {
    assert_eq!(extension("notes.txt").as_deref(), Some(".txt"));
    assert_eq!(extension("Report.PDF").as_deref(), Some(".pdf"));
    assert_eq!(extension("archive.tar.gz").as_deref(), Some(".gz"));
    assert_eq!(extension("Makefile"), None);
    assert_eq!(extension(".bashrc"), None);
    for locale in LOCALES {
        assert!(select_title(locale, "notes.txt").contains("notes.txt"));
        assert!(always_use(locale, &FileKind::Extension(".txt".into())).contains(".txt"));
        assert!(!always_use(locale, &FileKind::Unknown).contains('.'));
        assert!(other_apps(locale, 71).contains("71"));
    }
}

#[test]
fn both_locales_fill_in_every_string_the_dialog_reads() {
    for locale in LOCALES {
        let c = copy(locale);
        for value in [
            c.open_with_title,
            c.executable_title,
            c.executable_subtitle,
            c.search_placeholder,
            c.other_apps,
            c.current_default,
            c.why,
            c.open,
            c.cancel,
            c.undo,
            c.loading_title,
            c.empty_title,
            c.empty_detail,
            c.no_matches_title,
            c.no_matches_detail,
            c.launch_failed,
            c.launch_failed_advice,
            c.association_written,
            c.association_failed,
            c.association_unchanged,
            c.association_rolled_back,
            c.executable_use_path,
            c.executable_browse,
            c.executable_browse_empty,
            c.executable_selected,
        ] {
            assert!(!value.trim().is_empty(), "{locale:?} has an empty string");
            assert!(!value.contains("render"), "{locale:?}: {value}");
        }
    }
    assert_ne!(copy(Locale::EnUs).open, copy(Locale::ZhTw).open);
}

#[test]
fn every_executable_refusal_and_association_warning_says_something() {
    let refusals = [
        ExecutableWarning::NoSingleExecutable {
            reason: NoCanonicalExecutable::Flatpak,
        },
        ExecutableWarning::DBusActivated,
        ExecutableWarning::ProgramNotFound {
            program: "editor".into(),
        },
        ExecutableWarning::NoExecLine,
        ExecutableWarning::ComplexArguments {
            program: "editor".into(),
            dropped: vec!["--new-window".into()],
        },
        ExecutableWarning::NotFound {
            path: "/usr/bin/nope".into(),
        },
        ExecutableWarning::NotExecutable {
            path: "/etc/hosts".into(),
        },
    ];
    for locale in LOCALES {
        for refusal in &refusals {
            let message =
                executable_message(locale, &ExecutableResolution::Refused(refusal.clone()));
            assert!(!message.trim().is_empty(), "{locale:?} {refusal:?}");
            assert!(!message.contains("D-Bus"), "{message}");
        }
        assert!(
            executable_message(locale, &ExecutableResolution::Resolved("/usr/bin/x".into()))
                .contains("/usr/bin/x")
        );
        use app_chooser_core::AssociationWarning::*;
        for warning in [
            ApplicationDoesNotDeclareType,
            ListedInRemovedAssociations,
            DuplicateDefaultKey,
        ] {
            assert!(!warning_message(locale, &warning).trim().is_empty());
        }
    }
}

#[test]
fn the_dialog_is_as_tall_as_what_it_holds_and_no_taller_than_the_cap() {
    let base = metrics::HEADER + metrics::CHECK_BOX + metrics::BUTTONS + metrics::LIST_PADDING;
    // Three lines and a titlebar: every part, nothing else.
    assert_eq!(
        dialog_height(3, true, false),
        base + metrics::TITLE_BAR + 3.0 * metrics::ROW
    );
    // Embedded, there is no titlebar.
    assert_eq!(
        dialog_height(3, false, false) + metrics::TITLE_BAR,
        dialog_height(3, true, false)
    );
    // Two lines is the floor, and the minimum window still holds them.
    assert_eq!(dialog_height(0, true, false), dialog_height(2, true, false));
    assert!(dialog_height(0, false, false) >= MIN_WINDOW_HEIGHT);
    // A long list stops at the cap and scrolls inside it.
    assert_eq!(dialog_height(80, true, false), MAX_DIALOG_HEIGHT);
    let limit = list_height_limit(true, false);
    assert!(
        base + metrics::TITLE_BAR + limit <= MAX_DIALOG_HEIGHT + 0.5,
        "the list's limit keeps the buttons inside the dialog"
    );
    assert!(list_height_limit(true, true) < limit);
    assert!(dialog_height(3, true, true) > dialog_height(3, true, false));
}

#[test]
fn the_system_locale_resolves_to_one_of_the_two_shipped_locales() {
    assert!(matches!(
        Locale::System.resolved(),
        Locale::EnUs | Locale::ZhTw
    ));
    assert_eq!(Locale::ZhTw.resolved(), Locale::ZhTw);
    assert!(Locale::ZhTw.entry_locale().is_some());
}

#[test]
fn a_search_that_matches_nothing_leaves_nothing_to_open() {
    let sections = sections(None);
    let editor = DesktopId::new("editor.desktop").unwrap();
    let all = lines(&sections, "", false);
    assert_eq!(visible_selection(&all, Some(&editor)), Some(editor.clone()));
    // The selected app scrolled out of a search: the first match instead.
    let players = lines(&sections, "pla", false);
    assert_eq!(
        visible_selection(&players, Some(&editor)).unwrap().as_str(),
        "player.desktop"
    );
    // Nothing matches: nothing is selected, so Open has nothing to open.
    let none = lines(&sections, "zzz", false);
    assert_eq!(visible_selection(&none, Some(&editor)), None);
}

#[test]
fn the_check_box_names_the_type_not_the_last_extension() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<mime-type xmlns="http://www.freedesktop.org/standards/shared-mime-info" type="application/x-compressed-tar">
  <comment>Tar archive (gzip-compressed)</comment>
  <comment xml:lang="zh_TW">Tar 封存檔 (gzip 壓縮)</comment>
  <comment xml:lang="zh_CN">Tar 归档文件（gzip 压缩）</comment>
</mime-type>"#;
    assert_eq!(
        mime_comment(xml, None).as_deref(),
        Some("Tar archive (gzip-compressed)")
    );
    assert_eq!(
        mime_comment(xml, Some("zh_TW")).as_deref(),
        Some("Tar 封存檔 (gzip 壓縮)")
    );
    assert_eq!(mime_comment(xml, Some("fr")), None);
    let described = file_kind(mime_comment(xml, None), "a.tar.gz");
    let label = always_use(Locale::EnUs, &described);
    assert!(label.contains("Tar archive (gzip-compressed)"), "{label}");
    assert!(!label.contains(".gz"), "{label}");
    assert!(
        always_use(
            Locale::ZhTw,
            &file_kind(mime_comment(xml, Some("zh_TW")), "a.tar.gz")
        )
        .contains("Tar 封存檔")
    );
    // A type with no description in the language falls back to the
    // extension, and a name with none to the words alone.
    assert_eq!(
        file_kind(None, "notes.txt"),
        FileKind::Extension(".txt".into())
    );
    assert_eq!(file_kind(None, "Makefile"), FileKind::Unknown);
    assert_eq!(
        mime_comment("<comment>Plain &amp; simple</comment>", None).as_deref(),
        Some("Plain & simple")
    );
}

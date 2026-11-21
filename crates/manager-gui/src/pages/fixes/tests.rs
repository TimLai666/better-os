//! The Fixes page's decisions: groups, words in both languages, which
//! buttons a fault offers, Home's item, the pane's count, and the search.
//! Reports come from `patch-core`'s engine run against a fixture machine in
//! a scratch directory, never the person's.

use std::collections::BTreeMap;

use patch_core::engine::{self, Report};
use patch_core::entry;
use patch_core::flatpak::AppId;
use patch_core::{CheckOutcome, EntryId, Records, Subject};
use patch_platform::fixture::{self, SEEDED_APP, Seed};

use super::model::{FixesFacts, RowAction, RowKey, RowResult, Tone, page_rows};
use super::state::{HostChoice, host_choice};
use crate::attention::Snapshot;
use crate::i18n::Locale;
use crate::navigation::Route;
use crate::pages::{self, NavBadge};

const DIR: &str = "~/.local/share/flatpak/overrides";

fn check(root: &std::path::Path, records: &Records, undone: &[RowKey]) -> Vec<Report> {
    let mut host = fixture::host(root);
    entry::all()
        .into_iter()
        .map(|entry| {
            let subjects: Vec<Subject> = undone
                .iter()
                .filter(|key| key.entry == entry.id())
                .map(|key| key.subject.clone())
                .collect();
            engine::check(entry, &mut host, records, &subjects)
        })
        .collect()
}

fn machine(seed: Seed) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fixture::seed(root.path(), seed).unwrap();
    root
}

fn electron_key() -> RowKey {
    RowKey {
        entry: EntryId::ElectronCursor,
        subject: Subject::FlatpakApp(AppId::parse(SEEDED_APP).unwrap()),
    }
}

fn font_key() -> RowKey {
    RowKey {
        entry: EntryId::FontconfigCache,
        subject: Subject::Machine,
    }
}

#[test]
fn a_test_build_never_checks_the_person_s_machine() {
    assert_eq!(
        host_choice(false),
        HostChoice::Fixture("/nonexistent/better-os-test-root".into())
    );
    assert_eq!(
        host_choice(true),
        HostChoice::Fixture("/nonexistent/better-os-test-root".into())
    );
}

#[test]
fn both_faults_are_found_with_their_words_and_a_fix_button() {
    let root = machine(Seed::BothFaults);
    let reports = check(root.path(), &Records::default(), &[]);
    let rows = page_rows(Locale::EnUs, &reports, &BTreeMap::new(), DIR);
    assert!(rows.applied.is_empty());
    let headers: Vec<&str> = rows.found.iter().map(|r| r.header.as_str()).collect();
    assert_eq!(
        headers,
        [
            "Text is blank in some apps",
            "The pointer is half size over GitHub Desktop"
        ]
    );
    let font = &rows.found[0];
    assert_eq!(
        font.cause,
        "The font cache is damaged and answers every font query with the same file."
    );
    assert_eq!(font.change, ["Rebuilds your own font cache: fc-cache -f"]);
    assert_eq!(font.undo_note, "This fix doesn't need undoing.");
    assert_eq!(font.actions, [RowAction::Fix]);
    assert_eq!(font.state, ("Found on this PC.".to_string(), Tone::Warning));

    let electron = &rows.found[1];
    assert_eq!(
        electron.change,
        [format!(
            "Writes a Flatpak override for this app only, so it runs through XWayland, in \
             {DIR}/{SEEDED_APP}: --socket=x11 --nosocket=fallback-x11 --nosocket=wayland \
             --env=ELECTRON_OZONE_PLATFORM_HINT=x11 --env=XDG_SESSION_TYPE=x11"
        )]
    );
    assert_eq!(
        electron.undo_note,
        "Undo removes exactly these settings and leaves anything else in that file alone."
    );
    assert_eq!(
        electron.takes_effect.as_deref(),
        Some("Takes effect the next time GitHub Desktop starts.")
    );
    assert!(electron.limits.contains(
        &"Verified on Flatpak apps only. Nothing is offered for an app installed another way."
    ));

    let zh = page_rows(Locale::ZhTw, &reports, &BTreeMap::new(), DIR);
    assert_eq!(zh.found[0].header, "有些應用程式的文字是空白的");
    assert_eq!(
        zh.found[0].cause,
        "字型快取損壞了，每次查字型都拿到同一個檔案。"
    );
    assert_eq!(zh.found[0].undo_note, "這項修正不需要復原。");
    assert_eq!(zh.found[1].header, "GitHub Desktop 的游標只有一半大");
    assert_eq!(
        zh.found[1].cause,
        "這個應用程式在 Wayland 下不會放大游標，改用 XWayland 執行就正常，但分享畫面時可能只看得到 X11 程式的視窗。"
    );
}

#[test]
fn all_known_problems_lists_every_entry_with_its_state() {
    let root = machine(Seed::Healthy);
    let reports = check(root.path(), &Records::default(), &[]);
    let rows = page_rows(Locale::EnUs, &reports, &BTreeMap::new(), DIR);
    assert!(rows.found.is_empty());
    assert!(rows.applied.is_empty());
    let all: Vec<(&str, &str)> = rows
        .all
        .iter()
        .map(|r| (r.header, r.state.0.as_str()))
        .collect();
    assert_eq!(
        all,
        [
            (
                "Text is blank in some apps",
                "This PC doesn't have this problem."
            ),
            (
                "The pointer is half size over Electron apps",
                "This PC doesn't have this problem."
            ),
            // Ticket 101's two entries, on a laptop that is not one of the
            // models they are for.
            ("One USB-C port stops charging", "Doesn't apply to this PC."),
            (
                "USB on one side stops working after sleep",
                "Doesn't apply to this PC."
            ),
        ]
    );
    assert_eq!(rows.all[1].limits.len(), 4);
    let zh = page_rows(Locale::ZhTw, &reports, &BTreeMap::new(), DIR);
    assert_eq!(zh.all[1].header, "Electron 應用程式的游標只有一半大");
    assert_eq!(zh.all[0].state.0, "這台電腦沒有這個問題。");
}

#[test]
fn a_machine_whose_check_cannot_run_says_why() {
    // The test build's root: no fontconfig answers, no session.
    let reports = check(
        std::path::Path::new("/nonexistent/better-os-test-root"),
        &Records::default(),
        &[],
    );
    let rows = page_rows(Locale::EnUs, &reports, &BTreeMap::new(), DIR);
    assert!(rows.found.is_empty());
    assert_eq!(
        rows.all[0].state,
        (
            "Can't check: fc-match isn't installed on this PC.".to_string(),
            Tone::Warning
        )
    );
    assert_eq!(
        rows.all[1].state.0,
        "Can't check: It isn't known whether this session is Wayland or X11."
    );
    let zh = page_rows(Locale::ZhTw, &reports, &BTreeMap::new(), DIR);
    assert_eq!(zh.all[0].state.0, "無法檢查：這台電腦沒有安裝 fc-match。");
}

#[test]
fn after_a_fix_the_row_says_how_its_check_went_and_offers_undo() {
    let root = machine(Seed::BothFaults);
    let mut host = fixture::host(root.path());
    let reports = check(root.path(), &Records::default(), &[]);
    let mut records = Records::default();
    let mut results = BTreeMap::new();
    for (report, key) in reports.iter().zip([font_key(), electron_key()]) {
        let entry = entry::by_id(report.entry);
        let finding = report.instances[0].finding.clone().unwrap();
        let outcome = engine::fix(entry, &finding, &mut host, 1).unwrap();
        assert_eq!(outcome.check, CheckOutcome::Passed);
        records.keep(entry, outcome.record);
        results.insert(key, RowResult::Fixed(outcome.check));
    }
    let reports = check(root.path(), &records, &[]);
    let rows = page_rows(Locale::EnUs, &reports, &results, DIR);
    assert!(rows.found.is_empty());
    // The font fix keeps no record and still says how it went.
    let applied: Vec<(&str, &str, Vec<RowAction>)> = rows
        .applied
        .iter()
        .map(|r| (r.header.as_str(), r.state.0.as_str(), r.actions.clone()))
        .collect();
    assert_eq!(
        applied,
        [
            ("Text is blank in some apps", "Fixed and checked.", vec![]),
            (
                "The pointer is half size over GitHub Desktop",
                "Fixed and checked.",
                vec![RowAction::Undo]
            ),
        ]
    );
    let zh = page_rows(Locale::ZhTw, &reports, &results, DIR);
    assert_eq!(zh.applied[1].state.0, "已修正，檢查通過。");
    assert_eq!(zh.applied[1].header, "GitHub Desktop 的游標只有一半大");
    // Home has nothing to say now.
    assert!(FixesFacts::of(&reports).found.is_empty());
}

#[test]
fn a_fix_that_did_not_help_says_so_and_offers_undo_or_another_try() {
    let results = BTreeMap::from([
        (font_key(), RowResult::Fixed(CheckOutcome::StillPresent)),
        (electron_key(), RowResult::Fixed(CheckOutcome::StillPresent)),
    ]);
    let root = machine(Seed::BothFaults);
    // The fixture's rebuild does nothing here, and the Electron override is
    // changed back by hand after the fix.
    std::fs::remove_file(root.path().join("fonts-rebuilt.tsv")).unwrap();
    let mut host = fixture::host(root.path());
    let reports = check(root.path(), &Records::default(), &[]);
    let mut records = Records::default();
    // The two faults this machine has (its USB-C entries do not apply).
    for report in reports.iter().filter(|r| !r.instances.is_empty()) {
        let entry = entry::by_id(report.entry);
        let finding = report.instances[0].finding.clone().unwrap();
        let outcome = engine::fix(entry, &finding, &mut host, 1).unwrap();
        records.keep(entry, outcome.record);
    }
    let path =
        patch_platform::Paths::under(root.path()).user_override(&AppId::parse(SEEDED_APP).unwrap());
    let text = std::fs::read_to_string(&path).unwrap();
    // Put back on Wayland by hand: the socket, the session type, the hint.
    std::fs::write(
        &path,
        text.replace("HINT=x11", "HINT=auto")
            .replace("!wayland;", "wayland;")
            .replace("XDG_SESSION_TYPE=x11", "XDG_SESSION_TYPE=wayland"),
    )
    .unwrap();
    let reports = check(root.path(), &records, &[]);
    let rows = page_rows(Locale::EnUs, &reports, &results, DIR);
    let found: Vec<(&str, Vec<RowAction>)> = rows
        .found
        .iter()
        .map(|r| (r.state.0.as_str(), r.actions.clone()))
        .collect();
    assert_eq!(
        found,
        [
            (
                "Applied, but the problem is still there.",
                vec![RowAction::Fix]
            ),
            (
                "Applied, but the problem is still there.",
                vec![RowAction::Undo, RowAction::UndoAll]
            ),
        ]
    );
    assert_eq!(rows.found[1].changed_elsewhere.len(), 3);
    assert!(rows.found[1].changed_elsewhere.contains(
        &"ELECTRON_OZONE_PLATFORM_HINT (--env=ELECTRON_OZONE_PLATFORM_HINT=auto): Changed by another program; confirm it to replace".to_string()
    ));
    // The font cache has no undo; without a record it is simply found again.
    assert_eq!(reports[0].instances[0].state, patch_core::EntryState::Found);
    let zh = page_rows(Locale::ZhTw, &reports, &results, DIR);
    assert_eq!(zh.found[0].state.0, "已套用，但問題還在。");
    assert!(zh.found[1].changed_elsewhere[0].ends_with("已由其他程式變更，確認後才會取代"));
}

#[test]
fn an_undone_fix_reads_undone_and_offers_the_fix_again() {
    let root = machine(Seed::BothFaults);
    let undone = [electron_key()];
    let reports = check(root.path(), &Records::default(), &undone);
    let results = BTreeMap::from([(electron_key(), RowResult::Undone { skipped: 0 })]);
    let rows = page_rows(Locale::EnUs, &reports, &results, DIR);
    let electron = rows.found.iter().find(|r| r.key == electron_key()).unwrap();
    assert_eq!(electron.state.0, "Undone.");
    assert_eq!(electron.actions, [RowAction::Fix]);
    let skipped = BTreeMap::from([(electron_key(), RowResult::Undone { skipped: 2 })]);
    let rows = page_rows(Locale::ZhTw, &reports, &skipped, DIR);
    let electron = rows.found.iter().find(|r| r.key == electron_key()).unwrap();
    assert_eq!(
        electron.state.0,
        "已復原。有 2 項被其他程式變更的設定維持原樣。"
    );
    // The fault is back, so Home and the pane count it again.
    assert!(
        FixesFacts::of(&reports)
            .found
            .iter()
            .any(|fault| fault.entry == EntryId::ElectronCursor)
    );
}

#[test]
fn a_fault_whose_fix_is_not_offered_says_why_and_offers_nothing() {
    for (scale, en, zh) in [
        (
            "1.25\n",
            "No fix offered: a screen uses fractional scaling, and through XWayland the app would be drawn blurry.",
            "不提供修正：有螢幕用分數縮放，改用 XWayland 執行會讓應用程式變模糊。",
        ),
        (
            "2 1\n",
            "No fix offered: your screens use different scales, and XWayland draws the app at one of them, so it would be blurry or the wrong size on the other. This hasn't been verified.",
            "不提供修正：你的螢幕縮放比例不一樣，XWayland 只會用其中一個比例畫，另一個螢幕上會變模糊或大小不對。這種情況還沒驗證過。",
        ),
    ] {
        let root = machine(Seed::BothFaults);
        std::fs::write(root.path().join("scale"), scale).unwrap();
        let reports = check(root.path(), &Records::default(), &[]);
        let rows = page_rows(Locale::EnUs, &reports, &BTreeMap::new(), DIR);
        let electron = rows.found.iter().find(|r| r.key == electron_key()).unwrap();
        assert_eq!(
            electron.state,
            ("No fix offered.".to_string(), Tone::Warning)
        );
        // The reason is where the cause is, read without opening the card.
        assert_eq!(electron.cause, en);
        assert!(electron.facts.contains(
            &"This app doesn't scale its pointer on Wayland; running it through XWayland fixes it, but its screen sharing may then show only X11 windows."
                .to_string()
        ));
        assert!(electron.actions.is_empty());
        let zh_rows = page_rows(Locale::ZhTw, &reports, &BTreeMap::new(), DIR);
        let electron = zh_rows
            .found
            .iter()
            .find(|r| r.key == electron_key())
            .unwrap();
        assert_eq!(electron.state.0, "不提供修正。");
        assert_eq!(electron.cause, zh);
        // Home and the pane count only what can be acted on: the font cache.
        let facts = FixesFacts::of(&reports);
        assert_eq!(facts.found.len(), 1);
        assert_eq!(facts.found[0].entry, EntryId::FontconfigCache);
    }
}

#[test]
fn an_app_whose_electron_cannot_be_found_is_listed_as_cannot_tell() {
    let root = machine(Seed::BothFaults);
    let active = root
        .path()
        .join("system-flatpak/app")
        .join(SEEDED_APP)
        .join("current/active");
    std::fs::remove_dir_all(active.join("files")).unwrap();
    let metadata = std::fs::read_to_string(active.join("metadata")).unwrap();
    std::fs::write(
        active.join("metadata"),
        metadata.replace("ELECTRON_OZONE_PLATFORM_HINT=auto\n", ""),
    )
    .unwrap();
    let reports = check(root.path(), &Records::default(), &[]);
    let rows = page_rows(Locale::EnUs, &reports, &BTreeMap::new(), DIR);
    let electron = rows.found.iter().find(|r| r.key == electron_key()).unwrap();
    assert_eq!(electron.state.0, "Can't tell.");
    assert!(
        electron
            .cause
            .starts_with("Can't tell: this app's Electron version wasn't found")
    );
    assert!(electron.actions.is_empty());
    let zh = page_rows(Locale::ZhTw, &reports, &BTreeMap::new(), DIR);
    let electron = zh.found.iter().find(|r| r.key == electron_key()).unwrap();
    assert_eq!(electron.state.0, "無法判斷。");
}

#[test]
fn every_row_under_problems_found_is_drawn_as_a_problem() {
    let root = machine(Seed::BothFaults);
    let undone = [electron_key()];
    let reports = check(root.path(), &Records::default(), &undone);
    let results = BTreeMap::from([(electron_key(), RowResult::Undone { skipped: 0 })]);
    let rows = page_rows(Locale::EnUs, &reports, &results, DIR);
    // The undone row's state line is neutral, and it is still a problem.
    let electron = rows.found.iter().find(|r| r.key == electron_key()).unwrap();
    assert_eq!(electron.state.1, Tone::Neutral);
    assert!(rows.found.iter().all(|row| row.found));
    assert!(rows.applied.iter().all(|row| !row.found));
}

#[test]
fn the_card_says_which_electron_the_app_uses() {
    let root = machine(Seed::BothFaults);
    let reports = check(root.path(), &Records::default(), &[]);
    let rows = page_rows(Locale::EnUs, &reports, &BTreeMap::new(), DIR);
    let electron = rows.found.iter().find(|r| r.key == electron_key()).unwrap();
    assert_eq!(electron.facts, ["Uses Electron 32.3.0."]);
    let zh = page_rows(Locale::ZhTw, &reports, &BTreeMap::new(), DIR);
    let electron = zh.found.iter().find(|r| r.key == electron_key()).unwrap();
    assert_eq!(electron.facts, ["使用 Electron 32.3.0。"]);
}

#[test]
fn off_gnome_the_entry_cannot_check_on_this_desktop_yet() {
    use super::model::state_line;
    let state = patch_core::EntryState::CannotCheck(patch_core::CannotCheck::NotOnThisDesktop);
    assert_eq!(
        state_line(Locale::EnUs, &state).0,
        "Can't check: this desktop doesn't report its display scale yet."
    );
    assert_eq!(
        state_line(Locale::ZhTw, &state).0,
        "無法檢查：這個桌面環境還不能提供螢幕縮放比例。"
    );
}

#[test]
fn the_page_says_only_settings_changing_fixes_can_be_undone() {
    let en = super::copy::copy(Locale::EnUs);
    let zh = super::copy::copy(Locale::ZhTw);
    assert_eq!(
        en.description,
        "Better OS checks this PC for known problems. A fix that changes your settings can be undone; a setting you change yourself afterwards is left as you made it."
    );
    assert_eq!(
        zh.description,
        "Better OS 會檢查這台電腦有沒有已知的問題。會變更你設定的修正都可以復原；修正之後你自己改過的設定，會保留你改的樣子。"
    );
}

#[test]
fn home_and_the_pane_count_the_faults_found() {
    let root = machine(Seed::BothFaults);
    let facts = FixesFacts::of(&check(root.path(), &Records::default(), &[]));
    assert_eq!(facts.found.len(), 2);
    let snapshot = Snapshot {
        fixes: facts,
        ..Snapshot::empty(Locale::EnUs)
    };
    let module = pages::module(Route::Fixes);
    assert_eq!((module.badge)(&snapshot), Some(NavBadge::Count(2)));
    let items = (module.attention)(&snapshot);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].header, "2 known problems found");
    assert_eq!(items[0].description, "Text is blank in some apps");
    assert_eq!(items[0].action_label, "View");
    assert_eq!(
        items[0].action,
        crate::attention::AttentionAction::Open(crate::model::Page::Fixes)
    );
    assert_eq!(items[0].row, crate::attention::row::FIXES_FOUND);

    let zh = Snapshot {
        locale: Locale::ZhTw,
        ..snapshot.clone()
    };
    let items = (module.attention)(&zh);
    assert_eq!(items[0].header, "找到 2 個已知問題");
    assert_eq!(items[0].action_label, "查看");

    let one = Snapshot {
        fixes: FixesFacts {
            found: snapshot.fixes.found[1..].to_vec(),
        },
        ..Snapshot::empty(Locale::EnUs)
    };
    let items = (module.attention)(&one);
    assert_eq!(items[0].header, "1 known problem found");
    assert_eq!(
        items[0].description,
        "The pointer is half size over GitHub Desktop"
    );

    let none = Snapshot::empty(Locale::EnUs);
    assert_eq!((module.badge)(&none), None);
    assert!((module.attention)(&none).is_empty());
}

#[test]
fn the_search_finds_the_page_check_again_and_each_entry() {
    for (locale, query, card) in [
        (Locale::EnUs, "check again", super::card::CHECK),
        (Locale::EnUs, "blank", super::card::FONT_CACHE),
        (Locale::EnUs, "pointer", super::card::ELECTRON_CURSOR),
        (Locale::ZhTw, "重新檢查", super::card::CHECK),
        (Locale::ZhTw, "修正", super::card::CHECK),
    ] {
        let index = pages::search_index(locale);
        let found = crate::search::find(&index, query);
        assert!(
            found
                .iter()
                .any(|entry| entry.route == Route::Fixes && entry.card == card),
            "{locale:?} {query}"
        );
    }
}

#[test]
fn the_page_is_built_and_named_fixes() {
    let module = pages::module(Route::Fixes);
    assert_eq!(module.name, "fixes");
    assert_eq!(module.status, pages::PageStatus::Built);
    assert_eq!(pages::title(Route::Fixes, Locale::ZhTw), "修正");
    assert_eq!(pages::title(Route::Fixes, Locale::EnUs), "Fixes");
}

// --- Ticket 101: the USB-C entries -----------------------------------------

mod usb {
    use super::*;
    use hardware_core::usb_recovery::{
        ChargerShown, PciAddress, RecoveryReply, Restored, UsbChoice, UsbRecoverySettings,
    };
    use patch_platform::fixture::UsbSeed;

    use super::super::model::page_rows_with;

    fn usb_machine(seed: UsbSeed) -> tempfile::TempDir {
        let root = machine(Seed::Healthy);
        fixture::seed_usb(root.path(), seed).unwrap();
        root
    }

    fn key(entry: EntryId) -> RowKey {
        RowKey {
            entry,
            subject: Subject::Machine,
        }
    }

    #[test]
    fn both_usb_faults_are_found_with_their_facts_in_both_languages() {
        let root = usb_machine(UsbSeed::Failing);
        let reports = check(root.path(), &Records::default(), &[]);
        let rows = page_rows(Locale::EnUs, &reports, &BTreeMap::new(), DIR);
        let usb = rows
            .found
            .iter()
            .find(|r| r.key == key(EntryId::UsbCInterface))
            .unwrap();
        assert_eq!(usb.header, "One USB-C port stops charging");
        assert_eq!(usb.state, ("Found on this PC.".to_string(), Tone::Warning));
        assert_eq!(usb.actions, vec![RowAction::Fix, RowAction::ResetUsbC]);
        assert!(usb.facts.contains(
            &"Since this PC started, the USB-C interface stopped answering 2 times, 1 of them after sleep."
                .to_string()
        ));
        assert_eq!(
            usb.change,
            vec![
                "After each sleep, resets the USB-C interface when it stopped answering (reloads the ucsi_acpi driver).",
                "Resets it now.",
            ]
        );
        assert_eq!(
            usb.undo_note,
            "Undo turns this off again. Nothing else changes."
        );
        let controller = rows
            .found
            .iter()
            .find(|r| r.key == key(EntryId::UsbController))
            .unwrap();
        // Stuck: no Reset USB-C, the facts say a restart.
        assert_eq!(controller.actions, vec![RowAction::Fix]);
        assert!(
            controller
                .facts
                .iter()
                .any(|f| f == "The USB controller at 0000:63:00.4 stopped working after sleep.")
        );
        assert!(
            controller
                .facts
                .iter()
                .any(|f| f == "Sleep failed 2 times because of it, the last 2 in a row.")
        );
        assert!(
            controller
                .facts
                .iter()
                .any(|f| f.starts_with("It's stuck, and only restarting the PC"))
        );

        let zh = page_rows(Locale::ZhTw, &reports, &BTreeMap::new(), DIR);
        let usb = zh
            .found
            .iter()
            .find(|r| r.key == key(EntryId::UsbCInterface))
            .unwrap();
        assert_eq!(usb.header, "其中一個 USB-C 連接埠不再充電");
        assert!(usb.facts.contains(
            &"這台電腦開機以來，USB-C 介面停止回應了 2 次，其中 1 次在睡眠後。".to_string()
        ));
        let controller = zh
            .found
            .iter()
            .find(|r| r.key == key(EntryId::UsbController))
            .unwrap();
        assert_eq!(controller.header, "睡眠之後，一側的 USB 停止運作");
        assert!(
            controller
                .facts
                .iter()
                .any(|f| f == "睡眠因此失敗了 2 次，最近連續 2 次。")
        );
        // Home counts both.
        let facts = FixesFacts::of(&reports);
        assert_eq!(
            facts.found.iter().map(|f| f.entry).collect::<Vec<_>>(),
            vec![EntryId::UsbCInterface, EntryId::UsbController]
        );
    }

    #[test]
    fn a_fixed_controller_card_offers_the_power_off_choice_with_its_safety() {
        let root = usb_machine(UsbSeed::Failing);
        let mut host = fixture::host(root.path());
        let reports = check(root.path(), &Records::default(), &[]);
        let report = reports
            .iter()
            .find(|r| r.entry == EntryId::UsbController)
            .unwrap();
        let finding = report.instances[0].finding.clone().unwrap();
        engine::fix(entry::by_id(EntryId::UsbController), &finding, &mut host, 1).unwrap();
        let reports = check(root.path(), &Records::default(), &[]);
        let settings = host.usb_settings();
        assert!(settings.controller_automatic && !settings.power_off_when_sleep_blocked);
        let rows = page_rows_with(Locale::EnUs, &reports, &BTreeMap::new(), DIR, &settings);
        let row = rows
            .found
            .iter()
            .find(|r| r.key == key(EntryId::UsbController))
            .unwrap();
        assert_eq!(row.state.0, "Applied, but the problem is still there.");
        assert_eq!(
            row.actions,
            vec![RowAction::Undo, RowAction::PowerOff(true)]
        );
        assert!(
            row.facts.contains(
                &"When sleep keeps failing with the lid closed, Better OS only notifies you."
                    .to_string()
            )
        );
        assert!(
            row.facts
                .iter()
                .any(|f| f.starts_with("It never turns off the PC while the lid is open"))
        );

        let chosen = settings.with(UsbChoice::PowerOffWhenSleepBlocked, true);
        let rows = page_rows_with(Locale::ZhTw, &reports, &BTreeMap::new(), DIR, &chosen);
        let row = rows
            .found
            .iter()
            .find(|r| r.key == key(EntryId::UsbController))
            .unwrap();
        assert_eq!(
            row.actions,
            vec![RowAction::Undo, RowAction::PowerOff(false)]
        );
        assert!(
            row.facts.contains(
                &"闔上螢幕後睡眠一直失敗時，Better OS 會關機。開著的應用程式裡沒存的內容會不見。"
                    .to_string()
            )
        );
        let _ = UsbRecoverySettings::default();
    }

    #[test]
    fn on_another_laptop_nothing_is_offered_and_the_list_says_why() {
        let root = usb_machine(UsbSeed::NotAffected);
        let reports = check(root.path(), &Records::default(), &[]);
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let rows = page_rows(locale, &reports, &BTreeMap::new(), DIR);
            assert!(
                rows.found.iter().chain(&rows.applied).all(|r| !matches!(
                    r.key.entry,
                    EntryId::UsbCInterface | EntryId::UsbController
                ))
            );
            let states: Vec<&str> = rows
                .all
                .iter()
                .filter(|r| matches!(r.entry, EntryId::UsbCInterface | EntryId::UsbController))
                .map(|r| r.state.0.as_str())
                .collect();
            let expected = if locale == Locale::ZhTw {
                "不適用於這台電腦。"
            } else {
                "Doesn't apply to this PC."
            };
            assert_eq!(states, vec![expected, expected]);
        }
    }

    #[test]
    fn a_reset_says_what_came_back_in_both_languages() {
        let reply = |restored, recent| RecoveryReply { restored, recent };
        let ucsi = |ports, charger| Restored::Ucsi {
            ports,
            expected_ports: 2,
            charger,
        };
        for (reply, en, zh, tone) in [
            (
                reply(ucsi(2, ChargerShown::Online), false),
                "Reset: both USB-C ports are back, and the charger is charging.",
                "已重設：兩個 USB-C 連接埠都恢復了，充電器也在充電。",
                Tone::Success,
            ),
            (
                reply(ucsi(1, ChargerShown::Online), false),
                "Reset, but only 1 of 2 USB-C ports came back.",
                "已重設，但 2 個 USB-C 連接埠只恢復了 1 個。",
                Tone::Warning,
            ),
            (
                reply(ucsi(2, ChargerShown::NotShown), true),
                "Reset, but the plugged-in charger still doesn't show on a USB-C port. This is the reset from a moment ago; it isn't run twice in a row.",
                "已重設，但接著的充電器還是沒有出現在 USB-C 連接埠上。 這是剛才那次重設的結果，不會連續重設兩次。",
                Tone::Warning,
            ),
            (
                reply(
                    Restored::Controller {
                        address: PciAddress::parse("0000:63:00.4").unwrap(),
                        bound: true,
                        root_hubs: 2,
                        devices: 1,
                    },
                    false,
                ),
                "Restarted: the USB controller is back, with 1 devices on it.",
                "已重新啟動：USB 控制器恢復了，上面接著 1 個裝置。",
                Tone::Success,
            ),
        ] {
            assert_eq!(
                super::super::model::reset_line(Locale::EnUs, &reply),
                (en.to_string(), tone)
            );
            assert_eq!(super::super::model::reset_line(Locale::ZhTw, &reply).0, zh);
        }
    }

    /// A reset that cleared the fault keeps saying what came back after the
    /// fault is gone, without the fix's change lines.
    #[test]
    fn a_reset_that_cleared_the_fault_still_says_what_came_back() {
        let root = usb_machine(UsbSeed::Failing);
        let mut host = fixture::host(root.path());
        let reply = host
            .usb_act(patch_core::Privileged::ReloadUcsi)
            .unwrap()
            .unwrap();
        let reports = check(root.path(), &Records::default(), &[]);
        let results = BTreeMap::from([(key(EntryId::UsbCInterface), RowResult::Reset(reply))]);
        let rows = page_rows(Locale::EnUs, &reports, &results, DIR);
        assert!(
            rows.found
                .iter()
                .all(|r| r.key != key(EntryId::UsbCInterface))
        );
        let row = rows
            .applied
            .iter()
            .find(|r| r.key == key(EntryId::UsbCInterface))
            .unwrap();
        assert_eq!(
            row.state,
            (
                "Reset: both USB-C ports are back, and the charger is charging.".to_string(),
                Tone::Success
            )
        );
        assert!(row.change.is_empty() && row.undo_note.is_empty() && row.actions.is_empty());
    }

    #[test]
    fn a_daemon_refusal_is_a_sentence_never_a_key() {
        use patch_core::ChangeError;
        for (error, en) in [
            (
                ChangeError::Refused("daemon.error.usb_restart_needed".into()),
                "The USB controller is stuck, and only restarting the PC brings it back.",
            ),
            (
                ChangeError::Refused("daemon.error.usb_nothing_to_reset".into()),
                "Nothing to reset: the USB-C ports are working.",
            ),
            (
                ChangeError::Refused("daemon.error.auth_cancelled".into()),
                "Authentication wasn't completed, so nothing changed.",
            ),
            (
                ChangeError::NotInstalled("better-manager-daemon"),
                "The Better OS privileged service isn't installed, so this can't be done.",
            ),
        ] {
            let text = super::super::model::usb_refusal(Locale::EnUs, &error);
            assert_eq!(text, en);
            assert!(!super::super::model::usb_refusal(Locale::ZhTw, &error).contains("daemon."));
        }
    }
}

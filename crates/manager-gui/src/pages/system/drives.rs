//! System's Drives section (ticket 127, ADR 0032): every drive Better Files
//! would list that has a filesystem UUID, each with its "Mount
//! automatically" switch. It shares the setting with Better Files because
//! both read the system's own files through `drive-mounts` and both change
//! it through `manager-daemon`'s `org.betteros.Drives1`. A drive switched
//! off, which Better Files' sidebar no longer shows, is reached here.

use better_ui::fluent::button::Button;
use better_ui::fluent::info_bar::{InfoBar, Severity};
use better_ui::fluent::settings_card::SettingsCard;
use better_ui::fluent::toggle::ToggleSwitch;
use better_ui::fluent::{BODY, label, palette};
use drive_mounts::choice::{self, FollowUp, Locked, SystemFiles};
use drive_mounts::client::{self, SetOutcome};
use drive_mounts::listing::{Drive, DriveKind};
use gpui::*;
use gpui_component::IconName;

use crate::app::ManagerApp;
use crate::i18n::Locale;
use crate::render::{CARD_GAP, FIRST_CARD_GAP, Row};

pub(crate) const CARD: &str = "system-drives";

#[derive(Default)]
pub(crate) struct DrivesState {
    /// `None` until the first read lands; `Err` when UDisks2 could not be
    /// asked.
    pub(crate) list: Option<Result<Vec<Drive>, String>>,
    /// The drive whose switch is being written (and maybe waiting for
    /// polkit), by its key.
    pub(crate) writing: Option<String>,
    /// The last change that did not happen, and over which drive.
    pub(crate) bar: Option<(String, SetOutcome)>,
    again: bool,
    task: Option<Task<()>>,
    write_task: Option<Task<()>>,
}

impl DrivesState {
    pub(crate) fn blocks_restart(&self) -> bool {
        self.writing.is_some()
    }
}

pub(crate) struct Copy {
    pub(crate) group: &'static str,
    pub(crate) description: &'static str,
    pub(crate) mount_automatically: &'static str,
    pub(crate) none: &'static str,
    /// `{reason}`.
    pub(crate) unreadable: &'static str,
    /// `{size}`.
    pub(crate) unnamed: &'static str,
    pub(crate) system_drive: &'static str,
    pub(crate) internal: &'static str,
    pub(crate) removable: &'static str,
    /// `{path}`.
    pub(crate) mounted_at: &'static str,
    pub(crate) not_mounted: &'static str,
    pub(crate) locked_encrypted: &'static str,
    pub(crate) failed: &'static str,
    pub(crate) failed_refused: &'static str,
    pub(crate) failed_timeout: &'static str,
    pub(crate) retry: &'static str,
    /// `{line}`.
    pub(crate) system_table: &'static str,
    /// `{reason}`.
    pub(crate) files_unreadable: &'static str,
    pub(crate) auth_note: &'static str,
    /// `{drive}` and `{reason}`.
    pub(crate) unchanged: &'static str,
    pub(crate) reason_not_allowed: &'static str,
    pub(crate) reason_absent: &'static str,
    pub(crate) reason_system_table: &'static str,
    pub(crate) reason_system_drive: &'static str,
    pub(crate) reason_not_filesystem: &'static str,
    pub(crate) reason_busy: &'static str,
    pub(crate) reason_malformed: &'static str,
    /// `{detail}`.
    pub(crate) reason_mount_failed: &'static str,
    /// `{detail}`.
    pub(crate) reason_after: &'static str,
}

static EN: Copy = Copy {
    group: "Drives",
    description: "Choose which drives are mounted when you sign in or plug them in. Better Files shows only mounted drives.",
    mount_automatically: "Mount automatically",
    none: "No drives to choose from were found.",
    unreadable: "The drives couldn't be read ({reason}).",
    unnamed: "{size} drive",
    system_drive: "System drive · always mounted",
    internal: "Internal",
    removable: "Removable",
    mounted_at: "Mounted at {path}",
    not_mounted: "Not mounted",
    locked_encrypted: "Encrypted · locked",
    failed: "Couldn't mount",
    failed_refused: "Couldn't mount: the system refused it",
    failed_timeout: "Couldn't mount: the drive didn't respond in time",
    retry: "Retry",
    system_table: "Set up in the system's mount table (/etc/fstab, line {line}), so it's changed there",
    files_unreadable: "The system's mount table couldn't be read ({reason}), so this can't be changed",
    auth_note: "You'll be asked for an administrator password once.",
    unchanged: "{drive} wasn't changed: {reason}",
    reason_not_allowed: "you aren't allowed to change this",
    reason_absent: "the drive isn't connected",
    reason_system_table: "it's set up in the system's mount table (/etc/fstab)",
    reason_system_drive: "it's the system drive",
    reason_not_filesystem: "it has no files a person keeps",
    reason_busy: "it's in use. Close what's open on it and try again",
    reason_malformed: "the system's mount table couldn't be read safely, so nothing was written",
    reason_mount_failed: "it couldn't be mounted, so it was left off ({detail})",
    reason_after: "it was saved, but {detail}",
};

static ZH_TW: Copy = Copy {
    group: "磁碟",
    description: "選擇登入或插入時要掛載哪些磁碟。Better Files 只會顯示已掛載的磁碟。",
    mount_automatically: "自動掛載",
    none: "找不到可以設定的磁碟。",
    unreadable: "無法讀取磁碟（{reason}）。",
    unnamed: "{size} 磁碟",
    system_drive: "系統磁碟・一律掛載",
    internal: "內接",
    removable: "外接",
    mounted_at: "掛載於 {path}",
    not_mounted: "未掛載",
    locked_encrypted: "已加密・已鎖定",
    failed: "無法掛載",
    failed_refused: "無法掛載：系統拒絕掛載",
    failed_timeout: "無法掛載：磁碟沒有及時回應",
    retry: "重試",
    system_table: "已在系統的掛載表（/etc/fstab 第 {line} 行）設定，請在那裡變更",
    files_unreadable: "無法讀取系統的掛載表（{reason}），所以無法變更",
    auth_note: "第一次變更時會要求輸入管理員密碼。",
    unchanged: "「{drive}」沒有變更：{reason}",
    reason_not_allowed: "你沒有權限變更這項設定",
    reason_absent: "找不到這顆磁碟",
    reason_system_table: "系統的掛載表（/etc/fstab）已經設定了這顆磁碟",
    reason_system_drive: "這是系統磁碟",
    reason_not_filesystem: "這顆磁碟沒有可存放檔案的檔案系統",
    reason_busy: "磁碟正在使用中，請關閉上面開著的檔案後再試一次",
    reason_malformed: "系統的掛載表無法安全讀取，所以沒有寫入任何內容",
    reason_mount_failed: "無法掛載，所以維持關閉（{detail}）",
    reason_after: "已儲存，但{detail}",
};

pub(crate) fn copy(locale: Locale) -> &'static Copy {
    match locale.resolved() {
        Locale::ZhTw => &ZH_TW,
        _ => &EN,
    }
}

/// Sizes in the kit's binary units, as Better Files and Windows show them.
pub(crate) fn bytes(size: u64) -> String {
    better_ui::fluent::words::bytes(size)
}

/// The drives this section lists: block drives with a UUID (the choice is
/// kept by UUID), never a phone or a share.
pub(crate) fn listed(drives: Vec<Drive>) -> Vec<Drive> {
    let mut drives: Vec<Drive> = drives
        .into_iter()
        .filter(|drive| {
            drive.uuid.is_some()
                && matches!(
                    drive.kind,
                    DriveKind::System
                        | DriveKind::Internal
                        | DriveKind::Removable
                        | DriveKind::Optical
                )
        })
        .collect();
    let rank = |kind: DriveKind| match kind {
        DriveKind::System => 0,
        DriveKind::Internal => 1,
        _ => 2,
    };
    drives.sort_by(|a, b| {
        rank(a.kind)
            .cmp(&rank(b.kind))
            .then_with(|| a.label.cmp(&b.label))
    });
    drives
}

pub(crate) fn name(drive: &Drive, c: &Copy) -> String {
    drive
        .label
        .clone()
        .unwrap_or_else(|| c.unnamed.replace("{size}", &bytes(drive.size)))
}

/// The line under a drive's name.
pub(crate) fn description(drive: &Drive, c: &Copy) -> String {
    let auto = drive.auto_mount.as_ref();
    match auto.and_then(|auto| auto.locked.as_ref()) {
        Some(Locked::SystemDrive) => return c.system_drive.to_string(),
        Some(Locked::SystemTable { line, .. }) => {
            return c.system_table.replace("{line}", &line.to_string());
        }
        Some(Locked::Unreadable(reason)) => return c.files_unreadable.replace("{reason}", reason),
        None => {}
    }
    let kind = if drive.kind == DriveKind::Internal {
        c.internal
    } else {
        c.removable
    };
    let state = match &drive.mount_point {
        Some(point) => c.mounted_at.replace("{path}", &point.display().to_string()),
        None if drive.locked => c.locked_encrypted.to_string(),
        None if choice::failed(drive) => match auto.and_then(|auto| auto.failure.as_deref()) {
            Some("timeout") => c.failed_timeout,
            Some(_) => c.failed_refused,
            None => c.failed,
        }
        .to_string(),
        None => c.not_mounted.to_string(),
    };
    format!("{} · {kind} · {state}", bytes(drive.size))
}

pub(crate) fn reason(outcome: &SetOutcome, c: &Copy) -> String {
    match outcome {
        SetOutcome::Done { .. } | SetOutcome::Cancelled => String::new(),
        SetOutcome::NotAllowed => c.reason_not_allowed.into(),
        SetOutcome::Absent => c.reason_absent.into(),
        SetOutcome::Refused(why) => match why.as_str() {
            "system_table" => c.reason_system_table,
            "system_drive" => c.reason_system_drive,
            _ => c.reason_not_filesystem,
        }
        .into(),
        SetOutcome::Busy(_) => c.reason_busy.into(),
        SetOutcome::Malformed(_) => c.reason_malformed.into(),
        SetOutcome::MountFailed(detail) => c.reason_mount_failed.replace("{detail}", detail),
        SetOutcome::Failed(detail) => detail.clone(),
    }
}

/// The system bus. A test build opens none.
fn system_bus() -> Option<zbus::blocking::Connection> {
    if cfg!(test) {
        None
    } else {
        zbus::blocking::Connection::system().ok()
    }
}

/// Reads every drive and its choice. A debug build takes the fixture
/// Better Files takes (`BETTER_FILES_TEST_DRIVES`), so a sandbox never shows
/// the machine's own drives.
fn read() -> Result<Vec<Drive>, String> {
    if cfg!(test) {
        return Ok(Vec::new());
    }
    let files = SystemFiles::for_window();
    if cfg!(debug_assertions)
        && let Some(path) = std::env::var_os(drive_mounts::listing::FIXTURE_VARIABLE)
    {
        let mut drives = drive_mounts::listing::fixture_drives(std::path::Path::new(&path));
        for drive in &mut drives {
            let failure = drive.auto_mount.take().and_then(|auto| auto.failure);
            drive.auto_mount = choice::setting(drive, &files);
            if let (Some(auto), Some(failure)) = (drive.auto_mount.as_mut(), failure) {
                auto.failure = Some(failure);
            }
        }
        return Ok(listed(drives));
    }
    use storage_platform::traits::DeviceControl as _;
    let devices = smol::block_on(async {
        storage_platform::udisks::UDisks2::connect()
            .await
            .map_err(|e| e.to_string())?
            .enumerate()
            .await
            .map_err(|e| e.to_string())
    })?;
    let mounts = files_platform::MountTable::from_env();
    let root = mounts
        .mounts()
        .iter()
        .find(|mount| mount.mount_point == std::path::Path::new("/"))
        .map(|mount| mount.source.clone());
    let mut drives = drive_mounts::listing::drives_from_devices(&devices, root.as_deref());
    choice::apply(&mut drives, &files);
    if let Some(bus) = system_bus() {
        for drive in &mut drives {
            if choice::failed(drive)
                && let Some(uuid) = drive.uuid.clone()
                && let Some(auto) = drive.auto_mount.as_mut()
            {
                auto.failure = client::mount_failure(&bus, &uuid);
            }
        }
    }
    Ok(listed(drives))
}

/// A removable drive turned on is mounted now; a mount UDisks2 made of a
/// drive turned off is unmounted (`drive_mounts::choice::follow_up`).
fn follow(drive: &Drive, on: bool) -> Option<String> {
    let follow = choice::follow_up(drive, on);
    if follow == FollowUp::Nothing || cfg!(test) {
        return None;
    }
    let address = storage_platform::model::DeviceAddress {
        object_path: drive.object_path.clone()?,
        device_path: drive.device_path.clone().unwrap_or_default(),
    };
    use storage_platform::traits::DeviceControl as _;
    smol::block_on(async {
        let udisks = storage_platform::udisks::UDisks2::connect()
            .await
            .map_err(|e| e.to_string())?;
        match follow {
            FollowUp::Mount => udisks.mount(&address).await.map(|_| ()),
            FollowUp::Unmount => udisks.unmount(&address).await,
            FollowUp::Nothing => Ok(()),
        }
        .map_err(|e| e.to_string())
    })
    .err()
}

impl ManagerApp {
    /// Reads the drives off the UI thread; one asked for meanwhile runs once
    /// this one lands.
    pub(crate) fn refresh_drives(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.system.drives;
        if state.task.is_some() {
            state.again = true;
            return;
        }
        state.task = Some(cx.spawn(async move |this, cx| {
            let list = cx.background_spawn(async move { read() }).await;
            let _ = this.update(cx, |app, cx| {
                let state = &mut app.system.drives;
                state.task = None;
                state.list = Some(list);
                if std::mem::take(&mut state.again) {
                    app.refresh_drives(cx);
                }
                cx.notify();
            });
        }));
    }

    /// Turns a drive's switch: the daemon writes the choice (polkit asks
    /// once), then the drive is mounted or unmounted as the person may.
    pub(crate) fn set_drive_auto_mount(&mut self, key: &str, on: bool, cx: &mut Context<Self>) {
        let state = &mut self.system.drives;
        if state.writing.is_some() {
            return;
        }
        let Some(Ok(list)) = &state.list else {
            return;
        };
        let Some(drive) = list.iter().find(|drive| drive.key == key).cloned() else {
            return;
        };
        let (Some(uuid), Some(auto)) = (drive.uuid.clone(), drive.auto_mount.as_ref()) else {
            return;
        };
        if auto.locked.is_some() {
            return;
        }
        state.writing = Some(drive.key.clone());
        state.bar = None;
        cx.notify();
        let c = copy(self.locale);
        let shown = name(&drive, c);
        state.write_task = Some(cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_spawn(async move {
                    let outcome = match system_bus() {
                        Some(bus) => client::set_mount_automatically(&bus, &uuid, on),
                        None => SetOutcome::Failed("the system bus is not reachable".into()),
                    };
                    match (&outcome, follow(&drive, on)) {
                        (SetOutcome::Done { .. }, Some(detail)) => {
                            SetOutcome::Failed(c.reason_after.replace("{detail}", &detail))
                        }
                        _ => outcome,
                    }
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                let state = &mut app.system.drives;
                state.writing = None;
                state.write_task = None;
                if !matches!(outcome, SetOutcome::Done { .. } | SetOutcome::Cancelled) {
                    state.bar = Some((shown, outcome));
                }
                app.refresh_drives(cx);
                cx.notify();
            });
        }));
    }

    pub(crate) fn drive_rows(&mut self, cx: &mut Context<Self>) -> Vec<Row> {
        let c = copy(self.locale);
        let p = palette(cx);
        let mut rows = vec![self.group_header(c.group, cx)];
        rows.push(Row::new(
            FIRST_CARD_GAP,
            label(
                format!("{} {}", c.description, c.auth_note),
                BODY,
                p.text_secondary,
            )
            .pb(px(4.0)),
        ));
        if let Some((drive, outcome)) = &self.system.drives.bar {
            let text = c
                .unchanged
                .replace("{drive}", drive)
                .replace("{reason}", &reason(outcome, c));
            let close = Self::act(cx, |this, _, cx| {
                this.system.drives.bar = None;
                cx.notify();
            });
            rows.push(Row::new(
                CARD_GAP,
                InfoBar::new("system-drives-bar", Severity::Error, text)
                    .locale(self.locale.words())
                    .on_close(close)
                    .into_any_element(),
            ));
        }
        let list = match &self.system.drives.list {
            None => return rows,
            Some(Err(error)) => {
                rows.push(Row::new(
                    CARD_GAP,
                    label(
                        c.unreadable.replace("{reason}", error),
                        BODY,
                        p.text_secondary,
                    ),
                ));
                return rows;
            }
            Some(Ok(list)) => list.clone(),
        };
        if list.is_empty() {
            rows.push(Row::new(CARD_GAP, label(c.none, BODY, p.text_secondary)));
            return rows;
        }
        let writing = self.system.drives.writing.clone();
        for (index, drive) in list.iter().enumerate() {
            let Some(auto) = drive.auto_mount.clone() else {
                continue;
            };
            let key = drive.key.clone();
            let busy = writing.is_some();
            let mut toggle = ToggleSwitch::new(
                ElementId::named_usize("system-drive-switch", index),
                auto.on,
            )
            .locale(self.locale.words())
            .disabled(auto.locked.is_some() || busy)
            .on_change(Self::act_with(cx, move |this, on: bool, _, cx| {
                this.set_drive_auto_mount(&key, on, cx);
            }));
            if writing.as_deref() == Some(drive.key.as_str()) {
                toggle = toggle.disabled(true);
            }
            let mut content = div().flex().items_center().gap(px(12.0));
            if choice::failed(drive) {
                let key = drive.key.clone();
                content = content.child(
                    Button::standard(ElementId::named_usize("system-drive-retry", index), c.retry)
                        .disabled(busy)
                        .on_click(Self::act(cx, move |this, _, cx| {
                            this.set_drive_auto_mount(&key, true, cx);
                        })),
                );
            }
            content = content.child(toggle);
            let icon = match drive.kind {
                DriveKind::Removable | DriveKind::Optical => IconName::ExternalLink,
                _ => IconName::HardDrive,
            };
            let card_ = SettingsCard::new(
                ElementId::named_usize("system-drive", index),
                name(drive, c),
            )
            .icon(icon)
            .description(description(drive, c))
            .content(content);
            // The first card is the one the search lands on.
            if index == 0 {
                rows.push(Row::card(CARD_GAP, CARD, card_));
            } else {
                rows.push(Row::new(CARD_GAP, card_));
            }
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: that brings gpui's own `test` attribute in.
    use super::{EN, ZH_TW, bytes, description, listed, name, reason};
    use drive_mounts::choice::{AutoMount, Locked};
    use drive_mounts::client::SetOutcome;
    use drive_mounts::listing::{Drive, DriveKind};

    fn drive(kind: DriveKind, uuid: Option<&str>, label: Option<&str>) -> Drive {
        Drive {
            key: format!("{kind:?}{uuid:?}"),
            label: label.map(str::to_string),
            kind,
            mount_point: None,
            object_path: None,
            device_path: None,
            drive_object: None,
            backing: None,
            locked: false,
            uuid: uuid.map(str::to_string),
            size: 512_000_000_000,
            ejectable: false,
            capacity: None,
            auto_mount: None,
            form: None,
        }
    }

    #[test]
    fn only_block_drives_with_a_uuid_are_listed_system_first() {
        let list = listed(vec![
            drive(DriveKind::Removable, Some("AB"), Some("Stick")),
            drive(DriveKind::Network, None, Some("Share")),
            drive(DriveKind::Internal, None, Some("No UUID")),
            drive(DriveKind::Internal, Some("01D9"), Some("Windows")),
            drive(DriveKind::System, Some("AA"), None),
        ]);
        let kinds: Vec<DriveKind> = list.iter().map(|d| d.kind).collect();
        assert_eq!(
            kinds,
            [DriveKind::System, DriveKind::Internal, DriveKind::Removable]
        );
    }

    #[test]
    fn each_state_has_its_words_in_both_languages() {
        for c in [&EN, &ZH_TW] {
            let mut windows = drive(DriveKind::Internal, Some("01D9"), Some("Windows"));
            windows.auto_mount = Some(AutoMount {
                on: true,
                locked: None,
                failure: Some("timeout".into()),
            });
            assert!(description(&windows, c).ends_with(c.failed_timeout));
            windows.auto_mount = Some(AutoMount {
                on: false,
                locked: Some(Locked::SystemTable {
                    line: 4,
                    mount_point: "/data".into(),
                }),
                failure: None,
            });
            assert!(description(&windows, c).contains('4'));
            let mut system = drive(DriveKind::System, Some("AA"), None);
            system.auto_mount = Some(AutoMount {
                on: true,
                locked: Some(Locked::SystemDrive),
                failure: None,
            });
            assert_eq!(description(&system, c), c.system_drive);
            assert_eq!(name(&system, c), c.unnamed.replace("{size}", "477 GB"));
            for outcome in [
                SetOutcome::NotAllowed,
                SetOutcome::Absent,
                SetOutcome::Refused("system_table".into()),
                SetOutcome::Busy("x".into()),
                SetOutcome::Malformed("x".into()),
                SetOutcome::MountFailed("x".into()),
            ] {
                assert!(!reason(&outcome, c).is_empty());
            }
        }
        assert_eq!(bytes(32_000_000_000), "29.8 GB");
        assert_eq!(bytes(512_000_000_000), "477 GB");
    }
}

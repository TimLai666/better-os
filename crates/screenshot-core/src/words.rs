//! Every word Better Screenshot shows or writes, in Taiwan Mandarin and
//! English.
//!
//! The interface's own words are Better OS's. The folder and file names are
//! GNOME Shell 46's translations (`po/zh_TW.po`), deliberately: they decide
//! where a picture lands, and a person coming from GNOME has their earlier
//! pictures there.

/// The two languages Better OS windows speak.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Locale {
    TraditionalChinese,
    #[default]
    English,
}

/// The words in a saved file's path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SaveWords {
    /// The folder under Pictures (GNOME's `_('Screenshots')`).
    pub screenshots_folder: &'static str,
    /// A picture's name, `{}` the timestamp (`_('Screenshot from %s')`).
    pub screenshot_from: &'static str,
    /// The folder under Videos (`_('Screencasts')`).
    pub screencasts_folder: &'static str,
    /// A recording's name, `{}` the timestamp (`_('Screencast from %d %t')`).
    pub screencast_from: &'static str,
}

impl SaveWords {
    pub fn of(locale: Locale) -> SaveWords {
        match locale {
            Locale::English => SaveWords {
                screenshots_folder: "Screenshots",
                screenshot_from: "Screenshot from {}",
                screencasts_folder: "Screencasts",
                screencast_from: "Screencast from {}",
            },
            // `po/zh_TW.po` of GNOME Shell 46.0. Its "Screencast from %d %t"
            // entry is the old one ending in ".webm" (line 1797), so GNOME
            // 46 names a recording in English under a zh-TW desktop.
            Locale::TraditionalChinese => SaveWords {
                screenshots_folder: "螢幕快照",
                screenshot_from: "螢幕快照 {}",
                screencasts_folder: "螢幕錄影",
                screencast_from: "Screencast from {}",
            },
        }
    }
}

/// The interface's words.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterfaceWords {
    pub selection: &'static str,
    pub screen: &'static str,
    pub window: &'static str,
    pub selection_tip: &'static str,
    pub screen_tip: &'static str,
    pub window_tip: &'static str,
    pub picture_tip: &'static str,
    pub recording_tip: &'static str,
    pub capture_tip: &'static str,
    pub pointer_tip: &'static str,
    pub close_tip: &'static str,
    /// The recording bar's stop button.
    pub stop_recording: &'static str,
    /// The tray item's title while recording, `{}` the time.
    pub recording_title: &'static str,
    /// Shown in window mode on an output with no window to take.
    pub no_windows_here: &'static str,
    /// Why nothing is saved while the administrator disabled saving.
    pub saving_disabled: &'static str,
    /// Why nothing is saved when whether saving is allowed could not be
    /// read; `{}` the reason.
    pub saving_unknown: &'static str,
}

impl InterfaceWords {
    pub fn of(locale: Locale) -> InterfaceWords {
        match locale {
            Locale::English => InterfaceWords {
                selection: "Selection",
                screen: "Screen",
                window: "Window",
                selection_tip: "Area Selection",
                screen_tip: "Screen Selection",
                window_tip: "Window Selection",
                picture_tip: "Take Screenshot",
                recording_tip: "Record Screen",
                capture_tip: "Capture",
                pointer_tip: "Show Pointer",
                close_tip: "Close",
                stop_recording: "Stop Recording",
                recording_title: "Recording the screen, {}",
                no_windows_here: "No windows on this screen",
                saving_disabled: "Saving to disk is turned off on this computer.",
                saving_unknown: "Nothing was saved, because whether saving to disk is allowed on this computer could not be read: {}",
            },
            Locale::TraditionalChinese => InterfaceWords {
                selection: "選取範圍",
                screen: "螢幕",
                window: "視窗",
                selection_tip: "擷取選取的範圍",
                screen_tip: "擷取整個螢幕",
                window_tip: "擷取單一視窗",
                picture_tip: "拍攝螢幕快照",
                recording_tip: "錄製螢幕",
                capture_tip: "擷取",
                pointer_tip: "顯示滑鼠指標",
                close_tip: "關閉",
                stop_recording: "停止錄影",
                recording_title: "正在錄製螢幕，{}",
                no_windows_here: "這個螢幕上沒有視窗",
                saving_disabled: "這台電腦已關閉存檔到磁碟的功能。",
                saving_unknown: "沒有存檔，因為讀不到這台電腦是否允許存檔到磁碟：{}",
            },
        }
    }
}

/// A notification's words.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Notice {
    /// The source line ("Screenshot").
    pub app_name: &'static str,
    pub title: String,
    pub body: String,
    /// "Show in Files", present when there is a file to show.
    pub show_in_files: Option<&'static str>,
    /// The icon name, as GNOME's sources use.
    pub icon: &'static str,
}

/// What a notification is about.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoticeKind {
    /// A picture was copied and saved (S7).
    PictureSaved,
    /// A picture was copied; saving is disabled (S6), or could not be known
    /// to be allowed (the reason, to append).
    PictureCopiedOnly(Option<String>),
    /// A picture was copied, and saving it failed with this reason.
    PictureNotSaved(String),
    /// A recording was saved (R7).
    RecordingSaved,
    /// The recording could not start (R8).
    RecordingFailedToStart(String),
    /// The disk filled (R8).
    RecordingOutOfSpace,
    /// The recording ended for another reason (R8).
    RecordingEnded(String),
    /// The compositor gave no picture.
    CaptureFailed(String),
}

impl Notice {
    pub fn of(kind: &NoticeKind, locale: Locale) -> Notice {
        let zh = locale == Locale::TraditionalChinese;
        let app_name = if zh { "螢幕快照" } else { "Screenshot" };
        let pick = |chinese: &str, english: &str| {
            if zh {
                chinese.to_string()
            } else {
                english.to_string()
            }
        };
        let show = Some(if zh {
            "在檔案中顯示"
        } else {
            "Show in Files"
        });
        let picture_icon = "screenshot-recorded-symbolic";
        let recording_icon = "screencast-recorded-symbolic";
        match kind {
            NoticeKind::PictureSaved => Notice {
                app_name,
                title: pick("已擷取螢幕快照", "Screenshot captured"),
                body: pick(
                    "可以從剪貼簿貼上這張圖片。",
                    "You can paste the image from the clipboard.",
                ),
                show_in_files: show,
                icon: picture_icon,
            },
            NoticeKind::PictureCopiedOnly(reason) => Notice {
                app_name,
                title: pick("已擷取螢幕快照", "Screenshot captured"),
                body: {
                    let body = pick(
                        "可以從剪貼簿貼上這張圖片。",
                        "You can paste the image from the clipboard.",
                    );
                    match reason {
                        Some(reason) => format!("{body} {reason}"),
                        None => body,
                    }
                },
                show_in_files: None,
                icon: picture_icon,
            },
            NoticeKind::PictureNotSaved(reason) => Notice {
                app_name,
                title: pick("螢幕快照已複製，但沒有儲存", "Screenshot copied, not saved"),
                body: if zh {
                    format!("可以從剪貼簿貼上這張圖片。無法儲存：{reason}")
                } else {
                    format!(
                        "You can paste the image from the clipboard. It could not be saved: {reason}"
                    )
                },
                show_in_files: None,
                icon: picture_icon,
            },
            NoticeKind::RecordingSaved => Notice {
                app_name,
                title: pick("已錄製螢幕錄影", "Screencast recorded"),
                body: pick("按這裡觀看影片。", "Click here to view the video."),
                show_in_files: show,
                icon: recording_icon,
            },
            NoticeKind::RecordingFailedToStart(reason) => Notice {
                app_name,
                title: pick("螢幕錄影無法開始", "Screencast failed to start"),
                body: reason.clone(),
                show_in_files: None,
                icon: recording_icon,
            },
            NoticeKind::RecordingOutOfSpace => Notice {
                app_name,
                title: pick(
                    "螢幕錄影中止：磁碟空間不足",
                    "Screencast ended: Out of disk space",
                ),
                body: pick("按這裡觀看已錄下的部分。", "Click here to view the video."),
                show_in_files: show,
                icon: recording_icon,
            },
            NoticeKind::RecordingEnded(reason) => Notice {
                app_name,
                title: pick("螢幕錄影意外結束", "Screencast ended unexpectedly"),
                body: reason.clone(),
                show_in_files: show,
                icon: recording_icon,
            },
            NoticeKind::CaptureFailed(reason) => Notice {
                app_name,
                title: pick("無法擷取螢幕", "The screen could not be captured"),
                body: reason.clone(),
                show_in_files: None,
                icon: picture_icon,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{InterfaceWords, Locale, Notice, NoticeKind, SaveWords};

    #[test]
    fn both_languages_have_every_word() {
        for locale in [Locale::English, Locale::TraditionalChinese] {
            let words = InterfaceWords::of(locale);
            for word in [
                words.selection,
                words.screen,
                words.window,
                words.selection_tip,
                words.screen_tip,
                words.window_tip,
                words.picture_tip,
                words.recording_tip,
                words.capture_tip,
                words.pointer_tip,
                words.close_tip,
                words.stop_recording,
                words.recording_title,
                words.no_windows_here,
                words.saving_disabled,
                words.saving_unknown,
            ] {
                assert!(!word.trim().is_empty());
            }
            assert!(words.recording_title.contains("{}"));
            let save = SaveWords::of(locale);
            assert!(save.screenshot_from.contains("{}"));
            assert!(save.screencast_from.contains("{}"));
        }
    }

    #[test]
    fn a_saved_picture_offers_its_folder_and_a_locked_down_one_does_not() {
        let saved = Notice::of(&NoticeKind::PictureSaved, Locale::English);
        assert_eq!(saved.title, "Screenshot captured");
        assert_eq!(saved.body, "You can paste the image from the clipboard.");
        assert_eq!(saved.show_in_files, Some("Show in Files"));
        let copied = Notice::of(
            &NoticeKind::PictureCopiedOnly(None),
            Locale::TraditionalChinese,
        );
        let unknown = Notice::of(
            &NoticeKind::PictureCopiedOnly(Some("why".into())),
            Locale::English,
        );
        assert!(unknown.body.ends_with(" why"));
        assert_eq!(copied.title, "已擷取螢幕快照");
        assert_eq!(copied.show_in_files, None);
    }

    #[test]
    fn recording_failures_say_which() {
        let space = Notice::of(&NoticeKind::RecordingOutOfSpace, Locale::English);
        assert_eq!(space.title, "Screencast ended: Out of disk space");
        let start = Notice::of(
            &NoticeKind::RecordingFailedToStart("no vp8enc".into()),
            Locale::TraditionalChinese,
        );
        assert_eq!(start.title, "螢幕錄影無法開始");
        assert_eq!(start.body, "no vp8enc");
    }
}

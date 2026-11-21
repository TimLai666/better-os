//! The lock screen's card bottom: media controls and notifications
//! (design/login-screen-spec.md sections 4.2 and 4.3, L4 (a)).
//!
//! Notifications that arrived since the screen locked, newest first, at most
//! three, then "{n} more". A notification marked private (its sender's
//! `x-better-os-private` hint, or the Notifications page's per-application
//! rule) shows its application and "New notification" only; so does every
//! notification while "Show their content" is off. With "Show notifications
//! on the lock screen" off the block is not shown at all. The rows are not
//! buttons: nothing is opened or answered from a locked screen.

use better_core::local_time::TimeZone;

use crate::clock::{HourCycle, TimeLocale, format_clock};
use crate::copy::{Language, fill};

/// The hint a sender sets to keep a notification's content off the lock
/// screen.
pub const PRIVATE_HINT: &str = "x-better-os-private";
/// The most rows shown.
pub const MAX_ROWS: usize = 3;

/// The Notifications page's two settings (ticket 238); both on by default.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockScreenNotifications {
    pub show: bool,
    pub show_content: bool,
}

impl Default for LockScreenNotifications {
    fn default() -> Self {
        Self {
            show: true,
            show_content: true,
        }
    }
}

/// One notification from the notification service's history.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LockNotification {
    pub app_name: String,
    /// The application's icon name or path, passed through to the view.
    pub app_icon: String,
    pub title: String,
    pub body: String,
    /// Seconds since the epoch.
    pub received_at: i64,
    /// The `x-better-os-private` hint or the application's rule.
    pub private: bool,
}

/// One row as the card draws it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationRow {
    pub app_name: String,
    pub app_icon: String,
    pub title: String,
    /// One line of the body; `None` when the content is hidden or empty.
    pub body: Option<String>,
    pub time: String,
}

/// The block under the media row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationsBlock {
    pub heading: &'static str,
    pub rows: Vec<NotificationRow>,
    /// 「還有 {n} 則通知」 / "{n} more notifications" when more than three
    /// arrived.
    pub more: Option<String>,
}

/// How the rows' times are written.
#[derive(Clone, Copy, Debug)]
pub struct RowClock {
    pub zone: TimeZone,
    pub locale: TimeLocale,
    pub cycle: HourCycle,
}

/// The first line of a body with its markup removed. Notification bodies
/// may carry the specification's small markup (`<b>`, `<i>`, `<u>`, `<a>`,
/// `<img>`) and its five entities.
pub fn first_line(body: &str) -> Option<String> {
    let mut text = String::with_capacity(body.len());
    let mut in_tag = false;
    for character in body.chars() {
        match character {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    let text = text
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .and_then(|line| shown_text(line, MAX_BODY_CHARS))
}

/// The longest application name, title, body line, artist and track shown.
pub const MAX_APP_NAME_CHARS: usize = 64;
pub const MAX_TITLE_CHARS: usize = 120;
pub const MAX_BODY_CHARS: usize = 200;
pub const MAX_ARTIST_CHARS: usize = 120;
pub const MAX_TRACK_CHARS: usize = 120;

/// Invisible characters that are not of General_Category `Cf` but still show
/// nothing and can make a name or a line look empty or different: the
/// combining grapheme joiner (U+034F, `Mn`) and the Hangul fillers (U+115F,
/// U+1160, U+3164, U+FFA0, `Lo`). U+180E is `Cf` since Unicode 6.3 and is
/// removed with the rest.
const INVISIBLE_NOT_FORMAT: &[char] = &['\u{034F}', '\u{115F}', '\u{1160}', '\u{3164}', '\u{FFA0}'];

/// The black flag that begins an emoji tag sequence (UTS #51, a subdivision
/// flag such as 🏴 + tags `gbeng` + cancel tag).
const TAG_BASE: char = '\u{1F3F4}';
const TAG_CANCEL: char = '\u{E007F}';

fn is_tag_spec(character: char) -> bool {
    ('\u{E0020}'..='\u{E007E}').contains(&character)
}

/// Which characters of `chars` are tags inside a valid emoji tag sequence:
/// the black flag, one or more tag characters, and the cancel tag.
fn tags_in_sequences(chars: &[char]) -> Vec<bool> {
    let mut kept = vec![false; chars.len()];
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == TAG_BASE {
            let mut end = index + 1;
            while end < chars.len() && is_tag_spec(chars[end]) {
                end += 1;
            }
            if end > index + 1 && end < chars.len() && chars[end] == TAG_CANCEL {
                for keep in &mut kept[index + 1..=end] {
                    *keep = true;
                }
                index = end;
            }
        }
        index += 1;
    }
    kept
}

/// Whether a character is removed before display: every character of
/// General_Category `Cf` (format: the bidirectional controls and isolates,
/// U+200B, U+2060 to U+2064, U+206A to U+206F, U+FEFF, U+180E, the tags, and
/// the rest), except the joiners U+200C and U+200D that scripts and emoji
/// need and the tags of a valid emoji tag sequence; and the invisible
/// characters in [`INVISIBLE_NOT_FORMAT`]. The categories are the Unicode
/// 17.0 tables of the `unicode-properties` crate the workspace already
/// locks.
fn is_hidden(character: char, in_tag_sequence: bool) -> bool {
    use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};
    if INVISIBLE_NOT_FORMAT.contains(&character) {
        return true;
    }
    // The whole Tags block, its unassigned points included, outside a valid
    // sequence.
    if ('\u{E0000}'..='\u{E007F}').contains(&character) {
        return !in_tag_sequence;
    }
    character.general_category() == GeneralCategory::Format
        && !matches!(character, '\u{200C}' | '\u{200D}')
        && !in_tag_sequence
}

/// Text from another program (a notification, a media player) as the lock
/// screen may show it: on one line, with control characters (`Cc`) made a
/// word break, every format character (`Cf`) and the other invisible
/// characters removed ([`is_hidden`]), runs of white space made one space,
/// and at most `max_chars` characters (the last one then an ellipsis).
/// `None` when nothing is left. Every notification and media text goes
/// through it, so a sender cannot reverse text, fake a second line, make a
/// name look empty, or fill the card.
pub fn shown_text(text: &str, max_chars: usize) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let tags = tags_in_sequences(&chars);
    let mut cleaned = String::with_capacity(text.len().min(max_chars * 4));
    let mut pending_space = false;
    for (character, in_tag_sequence) in chars.iter().copied().zip(tags) {
        if character.is_whitespace() || character.is_control() {
            // A newline, a tab or any other control is a word break at most.
            pending_space = true;
            continue;
        }
        if is_hidden(character, in_tag_sequence) {
            continue;
        }
        if pending_space && !cleaned.is_empty() {
            cleaned.push(' ');
        }
        pending_space = false;
        cleaned.push(character);
    }
    if cleaned.is_empty() || max_chars == 0 {
        return None;
    }
    if cleaned.chars().count() > max_chars {
        let mut shortened: String = cleaned.chars().take(max_chars - 1).collect();
        shortened.truncate(shortened.trim_end().len());
        shortened.push('…');
        return Some(shortened);
    }
    Some(cleaned)
}

/// The block, or `None` when nothing is to be shown: the setting is off, or
/// nothing arrived since the screen locked.
pub fn notifications_block(
    notifications: &[LockNotification],
    locked_at: i64,
    settings: LockScreenNotifications,
    language: Language,
    clock: RowClock,
) -> Option<NotificationsBlock> {
    if !settings.show {
        return None;
    }
    let mut arrived: Vec<&LockNotification> = notifications
        .iter()
        .filter(|notification| notification.received_at >= locked_at)
        .collect();
    if arrived.is_empty() {
        return None;
    }
    arrived.sort_by_key(|notification| std::cmp::Reverse(notification.received_at));
    let copy = language.copy();
    let rows = arrived
        .iter()
        .take(MAX_ROWS)
        .map(|notification| {
            let hidden = notification.private || !settings.show_content;
            let time = format_clock(
                &clock.zone.at(notification.received_at),
                language,
                clock.locale,
                clock.cycle,
            )
            .short_time;
            NotificationRow {
                app_name: shown_text(&notification.app_name, MAX_APP_NAME_CHARS)
                    .unwrap_or_default(),
                app_icon: notification.app_icon.clone(),
                title: if hidden {
                    copy.new_notification.to_string()
                } else {
                    shown_text(&notification.title, MAX_TITLE_CHARS).unwrap_or_default()
                },
                body: if hidden {
                    None
                } else {
                    first_line(&notification.body)
                },
                time,
            }
        })
        .collect();
    let more = match arrived.len().saturating_sub(MAX_ROWS) {
        0 => None,
        1 => Some(copy.more_notifications_one.to_string()),
        n => Some(fill(copy.more_notifications, &[("n", &n.to_string())])),
    };
    Some(NotificationsBlock {
        heading: copy.while_you_were_away,
        rows,
        more,
    })
}

/// MPRIS's `PlaybackStatus`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

impl PlaybackStatus {
    pub fn from_mpris(value: &str) -> Option<Self> {
        match value {
            "Playing" => Some(Self::Playing),
            "Paused" => Some(Self::Paused),
            "Stopped" => Some(Self::Stopped),
            _ => None,
        }
    }
}

/// What an MPRIS player reports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaPlayer {
    pub status: PlaybackStatus,
    /// `xesam:title`.
    pub title: String,
    /// `xesam:artist`.
    pub artists: Vec<String>,
    /// `mpris:artUrl`.
    pub art_url: Option<String>,
    pub can_go_previous: bool,
    pub can_go_next: bool,
    pub can_play: bool,
    pub can_pause: bool,
}

/// What the middle button does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayPause {
    Play,
    Pause,
}

/// The media row as the card draws it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaRow {
    /// The screen reader's heading.
    pub heading: &'static str,
    /// `None` when the player gives none; the row then shows the artist only.
    pub title: Option<String>,
    pub artist: Option<String>,
    pub art_url: Option<String>,
    pub previous: Option<&'static str>,
    pub play_pause: Option<(PlayPause, &'static str)>,
    pub next: Option<&'static str>,
}

/// The row, shown while a player is playing or paused.
pub fn media_row(player: Option<&MediaPlayer>, language: Language) -> Option<MediaRow> {
    let player = player?;
    if player.status == PlaybackStatus::Stopped {
        return None;
    }
    let copy = language.copy();
    let title = shown_text(&player.title, MAX_TRACK_CHARS);
    let artists: Vec<String> = player
        .artists
        .iter()
        .filter_map(|artist| shown_text(artist, MAX_ARTIST_CHARS))
        .collect();
    let artist = (!artists.is_empty())
        .then(|| artists.join(", "))
        .and_then(|joined| shown_text(&joined, MAX_ARTIST_CHARS));
    let play_pause = match player.status {
        PlaybackStatus::Playing => player.can_pause.then_some((PlayPause::Pause, copy.pause)),
        _ => player.can_play.then_some((PlayPause::Play, copy.play)),
    };
    Some(MediaRow {
        heading: copy.now_playing,
        title,
        artist,
        art_url: player.art_url.clone(),
        previous: player.can_go_previous.then_some(copy.previous),
        play_pause,
        next: player.can_go_next.then_some(copy.next),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCKED_AT: i64 = 1_795_246_000;
    const CLOCK: RowClock = RowClock {
        zone: TimeZone::Fixed(8 * 3600),
        locale: TimeLocale::ZhTw,
        cycle: HourCycle::H12,
    };

    fn notification(title: &str, at: i64, private: bool) -> LockNotification {
        LockNotification {
            app_name: "Better Git".into(),
            app_icon: "io.betteros.Git".into(),
            title: title.into(),
            body: format!("<b>{title}</b> body &amp; more\nsecond line"),
            received_at: at,
            private,
        }
    }

    #[test]
    fn at_most_three_newest_first_then_the_rest_counted() {
        let list: Vec<_> = (0..5)
            .map(|index| notification(&format!("n{index}"), LOCKED_AT + index * 60, false))
            .chain([notification("before the lock", LOCKED_AT - 1, false)])
            .collect();
        let block = notifications_block(
            &list,
            LOCKED_AT,
            LockScreenNotifications::default(),
            Language::ZhTw,
            CLOCK,
        )
        .unwrap();
        assert_eq!(block.heading, "你離開時收到的通知");
        let titles: Vec<&str> = block.rows.iter().map(|row| row.title.as_str()).collect();
        assert_eq!(titles, ["n4", "n3", "n2"]);
        assert_eq!(block.rows[0].body.as_deref(), Some("n4 body & more"));
        assert_eq!(block.more.as_deref(), Some("還有 2 則通知"));
        assert!(block.rows[0].time.starts_with("下午"));
    }

    #[test]
    fn one_more_is_said_in_the_singular() {
        let list: Vec<_> = (0..4)
            .map(|index| notification("x", LOCKED_AT + index, false))
            .collect();
        let block = notifications_block(
            &list,
            LOCKED_AT,
            LockScreenNotifications::default(),
            Language::EnUs,
            CLOCK,
        )
        .unwrap();
        assert_eq!(block.more.as_deref(), Some("1 more notification"));
        let five: Vec<_> = (0..5)
            .map(|index| notification("x", LOCKED_AT + index, false))
            .collect();
        let block = notifications_block(
            &five,
            LOCKED_AT,
            LockScreenNotifications::default(),
            Language::EnUs,
            CLOCK,
        )
        .unwrap();
        assert_eq!(block.more.as_deref(), Some("2 more notifications"));
    }

    #[test]
    fn a_private_notification_shows_only_its_application() {
        let list = [notification("王大同的訊息", LOCKED_AT + 5, true)];
        let block = notifications_block(
            &list,
            LOCKED_AT,
            LockScreenNotifications::default(),
            Language::ZhTw,
            CLOCK,
        )
        .unwrap();
        assert_eq!(block.rows[0].app_name, "Better Git");
        assert_eq!(block.rows[0].title, "新通知");
        assert_eq!(block.rows[0].body, None);
        assert_eq!(block.more, None);
    }

    #[test]
    fn the_two_settings() {
        let list = [notification("secret plans", LOCKED_AT + 5, false)];
        let hidden_content = LockScreenNotifications {
            show: true,
            show_content: false,
        };
        let block =
            notifications_block(&list, LOCKED_AT, hidden_content, Language::EnUs, CLOCK).unwrap();
        assert_eq!(block.rows[0].title, "New notification");
        assert_eq!(block.rows[0].body, None);
        let off = LockScreenNotifications {
            show: false,
            show_content: true,
        };
        assert_eq!(
            notifications_block(&list, LOCKED_AT, off, Language::EnUs, CLOCK),
            None
        );
        assert_eq!(
            notifications_block(
                &[],
                LOCKED_AT,
                LockScreenNotifications::default(),
                Language::EnUs,
                CLOCK
            ),
            None
        );
    }

    #[test]
    fn markup_and_blank_lines_are_removed_from_the_body() {
        assert_eq!(
            first_line("\n  <i>hi</i> &lt;there&gt;  \nmore").as_deref(),
            Some("hi <there>")
        );
        assert_eq!(first_line("<img src='x'/>"), None);
        assert_eq!(first_line(""), None);
        assert_eq!(first_line("a &amp;lt; b").as_deref(), Some("a &lt; b"));
    }

    fn player(status: PlaybackStatus) -> MediaPlayer {
        MediaPlayer {
            status,
            title: "午後散步".into(),
            artists: vec!["示意樂團".into()],
            art_url: Some("file:///tmp/cover.png".into()),
            can_go_previous: true,
            can_go_next: false,
            can_play: true,
            can_pause: true,
        }
    }

    #[test]
    fn the_media_row_while_playing_or_paused() {
        let row = media_row(Some(&player(PlaybackStatus::Playing)), Language::ZhTw).unwrap();
        assert_eq!(row.heading, "正在播放");
        assert_eq!(row.title.as_deref(), Some("午後散步"));
        assert_eq!(row.artist.as_deref(), Some("示意樂團"));
        assert_eq!(row.play_pause, Some((PlayPause::Pause, "暫停")));
        assert_eq!(row.previous, Some("上一首"));
        assert_eq!(row.next, None);
        let paused = media_row(Some(&player(PlaybackStatus::Paused)), Language::EnUs).unwrap();
        assert_eq!(paused.play_pause, Some((PlayPause::Play, "Play")));
        assert_eq!(
            media_row(Some(&player(PlaybackStatus::Stopped)), Language::EnUs),
            None
        );
        assert_eq!(media_row(None, Language::EnUs), None);
        let mut untitled = player(PlaybackStatus::Playing);
        untitled.title = " ".into();
        untitled.artists.clear();
        let row = media_row(Some(&untitled), Language::EnUs).unwrap();
        assert_eq!((row.title, row.artist), (None, None));
        assert_eq!(
            PlaybackStatus::from_mpris("Paused"),
            Some(PlaybackStatus::Paused)
        );
        assert_eq!(PlaybackStatus::from_mpris("paused"), None);
    }

    #[test]
    fn shown_text_removes_what_could_fake_or_hide_text() {
        // A newline in a title cannot make a second line.
        assert_eq!(
            shown_text("Update\nSystem: enter password", 120).as_deref(),
            Some("Update System: enter password")
        );
        // U+202E would show "gnp.exe" backwards; the overrides go.
        assert_eq!(
            shown_text("photo\u{202E}gnp.exe", 120).as_deref(),
            Some("photognp.exe")
        );
        assert_eq!(
            shown_text(
                "a\u{2066}b\u{2069}c\u{200F}d\u{061C}e\u{FEFF}f\u{200B}g",
                120
            )
            .as_deref(),
            Some("abcdefg")
        );
        assert_eq!(
            shown_text("tab\there\u{7}bell\u{85}next", 120).as_deref(),
            Some("tab here bell next")
        );
        // Joiners stay: a family emoji is one picture.
        assert_eq!(
            shown_text("👨\u{200D}👩", 120).as_deref(),
            Some("👨\u{200D}👩")
        );
        assert_eq!(shown_text("  \n\u{202E} ", 120), None);
        assert_eq!(shown_text("x", 0), None);
    }

    /// The ranges the review listed, every character of them: tags outside a
    /// sequence, U+206A to U+206F, U+180E, U+034F and the Hangul fillers.
    #[test]
    fn every_listed_invisible_character_is_removed() {
        let ranges: &[(u32, u32)] = &[
            (0xE0000, 0xE007F),
            (0x206A, 0x206F),
            (0x180E, 0x180E),
            (0x034F, 0x034F),
            (0x115F, 0x1160),
            (0x3164, 0x3164),
            (0xFFA0, 0xFFA0),
            (0x202A, 0x202E),
            (0x2066, 0x2069),
            (0x200B, 0x200B),
            (0x200E, 0x200F),
            (0x2060, 0x2064),
            (0xFEFF, 0xFEFF),
            (0xFFF9, 0xFFFB),
            (0x00AD, 0x00AD),
            (0x061C, 0x061C),
        ];
        for &(start, end) in ranges {
            for code in start..=end {
                let Some(character) = char::from_u32(code) else {
                    continue;
                };
                assert_eq!(
                    shown_text(&format!("a{character}b"), 120).as_deref(),
                    Some("ab"),
                    "U+{code:04X}"
                );
            }
        }
        // A name made only of fillers is no name.
        assert_eq!(shown_text("\u{3164}\u{115F}", 120), None);
        // Variation selectors are not format characters: an emoji keeps them.
        assert_eq!(shown_text("❤\u{FE0F}", 120).as_deref(), Some("❤\u{FE0F}"));
    }

    #[test]
    fn a_valid_emoji_tag_sequence_keeps_its_tags() {
        // England's flag: black flag, tags "gbeng", cancel tag.
        let england = "\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}";
        assert_eq!(
            shown_text(&format!("go {england}!"), 120).as_deref(),
            Some(format!("go {england}!").as_str())
        );
        // Without the cancel tag, or without the flag, the tags go.
        assert_eq!(
            shown_text("\u{1F3F4}\u{E0067}\u{E0062}x", 120).as_deref(),
            Some("\u{1F3F4}x")
        );
        assert_eq!(
            shown_text("a\u{E0067}\u{E007F}b", 120).as_deref(),
            Some("ab")
        );
    }

    #[test]
    fn shown_text_is_capped() {
        let long = "字".repeat(500);
        let shown = shown_text(&long, MAX_TITLE_CHARS).unwrap();
        assert_eq!(shown.chars().count(), MAX_TITLE_CHARS);
        assert!(shown.ends_with('…'));
        assert_eq!(shown_text("exactly", 7).as_deref(), Some("exactly"));
        assert_eq!(shown_text("one two three", 8).as_deref(), Some("one two…"));
    }

    #[test]
    fn every_notification_and_media_text_is_cleaned_and_capped() {
        let list = [LockNotification {
            app_name: format!("App\u{202E}{}", "a".repeat(100)),
            app_icon: "icon".into(),
            title: "Line one\nLine two".into(),
            body: format!("\u{202E}{}", "b".repeat(400)),
            received_at: LOCKED_AT + 1,
            private: false,
        }];
        let block = notifications_block(
            &list,
            LOCKED_AT,
            LockScreenNotifications::default(),
            Language::EnUs,
            CLOCK,
        )
        .unwrap();
        let row = &block.rows[0];
        assert_eq!(row.app_name.chars().count(), MAX_APP_NAME_CHARS);
        assert!(!row.app_name.contains('\u{202E}'));
        assert_eq!(row.title, "Line one Line two");
        let body = row.body.as_deref().unwrap();
        assert_eq!(body.chars().count(), MAX_BODY_CHARS);
        assert!(!body.contains('\u{202E}'));

        let mut long = player(PlaybackStatus::Playing);
        long.title = format!("Track\u{202E}\n{}", "t".repeat(300));
        long.artists = vec!["A\u{2067}".into(), "c".repeat(300)];
        let row = media_row(Some(&long), Language::EnUs).unwrap();
        let title = row.title.unwrap();
        assert!(title.starts_with("Track t"));
        assert_eq!(title.chars().count(), MAX_TRACK_CHARS);
        let artist = row.artist.unwrap();
        assert!(artist.starts_with("A, c"));
        assert_eq!(artist.chars().count(), MAX_ARTIST_CHARS);
    }
}

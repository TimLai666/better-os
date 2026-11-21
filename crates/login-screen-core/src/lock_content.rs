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
        .map(str::to_string)
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
                app_name: notification.app_name.clone(),
                app_icon: notification.app_icon.clone(),
                title: if hidden {
                    copy.new_notification.to_string()
                } else {
                    notification.title.clone()
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
    let title = Some(player.title.trim())
        .filter(|title| !title.is_empty())
        .map(str::to_string);
    let artists: Vec<&str> = player
        .artists
        .iter()
        .map(|artist| artist.trim())
        .filter(|artist| !artist.is_empty())
        .collect();
    let artist = (!artists.is_empty()).then(|| artists.join(", "));
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
}

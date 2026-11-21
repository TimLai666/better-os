//! The layout rules both screens draw by (design/login-screen-spec.md
//! sections 2, 3.1, 3.4, 3.11, 3.14 and 1.2's leaving motion), as numbers a
//! GPUI view reads. Every number is in logical pixels.
//!
//! - **Wide**, 800 and wider: the clock group at the bottom start (64 from the
//!   start edge, 56 from the bottom), the card at the end edge (20 from the
//!   top, end and bottom), 480 wide at 1100 and wider and 440 below. The
//!   clock is weekday 120 over time 56, and 88 over 44 below 1100.
//! - **Narrow**, below 800: the quiet view's clock centred at 50% of the
//!   height, rising to 23% as the entry view comes in, the entry rising 48
//!   into the lower 60%; the time is `clamp(48, width / 4.8, 96)`.
//! - **Large text**: every text size × 1.25, and the card 560 wide. The wide
//!   layout then needs 560 for the card and 1.25 × 360 for the clock group,
//!   so below 1010 the screen is narrow.
//! - **Right to left**: start and end swap; the card is at the left edge.

/// Where the wide layout begins.
pub const WIDE_MIN_WIDTH: f32 = 800.0;
/// From here the card is 480 and the clock full size.
pub const FULL_CLOCK_MIN_WIDTH: f32 = 1100.0;
pub const CARD_WIDTH: f32 = 480.0;
pub const CARD_WIDTH_COMPACT: f32 = 440.0;
pub const CARD_WIDTH_LARGE_TEXT: f32 = 560.0;
pub const CARD_INSET: f32 = 20.0;
pub const CARD_PADDING: f32 = 24.0;
/// A surface that floats (`DESIGN.md`).
pub const CARD_RADIUS: f32 = 8.0;
pub const CLOCK_START_INSET: f32 = 64.0;
pub const CLOCK_BOTTOM_INSET: f32 = 56.0;
/// The clock group's narrowest useful width, which with the card's sets the
/// wide threshold (section 2).
pub const CLOCK_MIN_WIDTH: f32 = 360.0;
pub const LARGE_TEXT_SCALE: f32 = 1.25;
/// The quiet view's hint, above the bottom edge.
pub const QUIET_HINT_BOTTOM: f32 = 40.0;
pub const NARROW_INSET: f32 = 16.0;
pub const NARROW_FIELD_MAX_WIDTH: f32 = 400.0;

/// Motion, in milliseconds and logical pixels. With animations off every
/// one is instant.
pub mod motion {
    /// The card coming in from the quiet view, moving in from the end.
    pub const CARD_ENTER_MS: u32 = 250;
    pub const CARD_ENTER_OFFSET: f32 = 24.0;
    /// Back to the quiet view after a minute without input.
    pub const QUIET_RETURN_MS: u32 = 200;
    /// Leaving after a successful login: the card moves to the end and fades.
    pub const CARD_LEAVE_MS: u32 = 600;
    pub const CARD_LEAVE_OFFSET: f32 = 24.0;
    /// The clock drops and fades.
    pub const CLOCK_LEAVE_MS: u32 = 400;
    pub const CLOCK_LEAVE_OFFSET: f32 = 12.0;
    /// The narrow entry view coming in.
    pub const NARROW_ENTER_MS: u32 = 250;
    /// How far the narrow entry rises into place.
    pub const NARROW_ENTRY_RISE: f32 = 48.0;
}

/// Reading direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    LeftToRight,
    RightToLeft,
}

/// A physical screen edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Edge {
    Left,
    Right,
}

impl Direction {
    /// The edge a line starts at.
    pub fn start(self) -> Edge {
        match self {
            Self::LeftToRight => Edge::Left,
            Self::RightToLeft => Edge::Right,
        }
    }

    pub fn end(self) -> Edge {
        match self {
            Self::LeftToRight => Edge::Right,
            Self::RightToLeft => Edge::Left,
        }
    }
}

/// A distance from a physical edge, for an absolutely placed element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inset {
    pub edge: Edge,
    pub by: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A text style: size over line height, and weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub line_height: f32,
    pub weight: u16,
}

impl TextStyle {
    const fn new(size: f32, line_height: f32, weight: u16) -> Self {
        Self {
            size,
            line_height,
            weight,
        }
    }

    fn scaled(self, scale: f32) -> Self {
        Self {
            size: self.size * scale,
            line_height: self.line_height * scale,
            weight: self.weight,
        }
    }
}

/// `DESIGN.md`'s ramp.
pub const CAPTION: TextStyle = TextStyle::new(12.0, 16.0, 400);
pub const BODY: TextStyle = TextStyle::new(14.0, 20.0, 400);
pub const BODY_STRONG: TextStyle = TextStyle::new(14.0, 20.0, 600);
pub const SUBTITLE: TextStyle = TextStyle::new(20.0, 28.0, 600);
pub const TITLE: TextStyle = TextStyle::new(28.0, 36.0, 600);

/// The clock group's display sizes, this screen's one exception to
/// "Nothing is larger than Title" (`DESIGN.md`).
pub const WEEKDAY: TextStyle = TextStyle::new(120.0, 120.0, 600);
pub const TIME: TextStyle = TextStyle::new(56.0, 64.0, 300);
pub const WEEKDAY_COMPACT: TextStyle = TextStyle::new(88.0, 88.0, 600);
pub const TIME_COMPACT: TextStyle = TextStyle::new(44.0, 52.0, 300);
pub const PERIOD: TextStyle = TextStyle::new(18.0, 24.0, 600);
pub const DATE: TextStyle = TextStyle::new(20.0, 28.0, 400);
pub const NARROW_DATE: TextStyle = TextStyle::new(17.0, 24.0, 400);

/// The type ramp at the screen's text scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ramp {
    pub caption: TextStyle,
    pub body: TextStyle,
    pub body_strong: TextStyle,
    pub subtitle: TextStyle,
    pub title: TextStyle,
}

impl Ramp {
    fn at(scale: f32) -> Self {
        Self {
            caption: CAPTION.scaled(scale),
            body: BODY.scaled(scale),
            body_strong: BODY_STRONG.scaled(scale),
            subtitle: SUBTITLE.scaled(scale),
            title: TITLE.scaled(scale),
        }
    }
}

/// The clock group of the wide layout. Its text is white in both
/// appearances over the scrim, with a 0 2 24 shadow at 25% black.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WideClock {
    /// From the start edge.
    pub horizontal: Inset,
    pub bottom: f32,
    pub weekday: TextStyle,
    /// Between the weekday and the row under it.
    pub row_gap: f32,
    pub time: TextStyle,
    pub period: TextStyle,
    /// Between the time and the period.
    pub period_gap: f32,
    /// 75% white.
    pub period_alpha: f32,
    /// The divider, 1 × 32 at 35% white, 16 from each side.
    pub divider_width: f32,
    pub divider_height: f32,
    pub divider_gap: f32,
    pub divider_alpha: f32,
    pub date: TextStyle,
    /// 90% white.
    pub date_alpha: f32,
}

/// The card's inside, top to bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardContent {
    pub padding: f32,
    pub status_row_height: f32,
    /// The status row's buttons, end-aligned.
    pub status_button: f32,
    pub status_button_gap: f32,
    pub status_row_edge: Edge,
    pub avatar: f32,
    pub avatar_to_name: f32,
    pub name: TextStyle,
    pub name_to_caption: f32,
    pub caption: TextStyle,
    pub identity_to_field: f32,
    pub field_width: f32,
    pub field_height: f32,
    /// The reveal and submit buttons inside the field's end.
    pub field_button: f32,
    pub field_to_message: f32,
    pub message: TextStyle,
    /// At most two lines.
    pub message_max_lines: u8,
    pub message_to_reserved: f32,
    /// Empty now; the fingerprint or smart card hint later (section 3.10).
    pub reserved_line_height: f32,
    pub bottom_row_height: f32,
    /// The account chooser's rows.
    pub chooser_row_height: f32,
    pub chooser_avatar: f32,
    /// The lock screen's notification rows.
    pub notification_row_height: f32,
    pub notification_icon: f32,
    pub media_cover: f32,
}

impl CardContent {
    fn new(ramp: &Ramp, direction: Direction, field_width: f32) -> Self {
        Self {
            padding: CARD_PADDING,
            status_row_height: 32.0,
            status_button: 32.0,
            status_button_gap: 4.0,
            status_row_edge: direction.end(),
            avatar: 96.0,
            avatar_to_name: 16.0,
            name: ramp.title,
            name_to_caption: 4.0,
            caption: ramp.caption,
            identity_to_field: 24.0,
            field_width,
            field_height: 36.0,
            field_button: 28.0,
            field_to_message: 8.0,
            message: ramp.caption,
            message_max_lines: 2,
            message_to_reserved: 4.0,
            reserved_line_height: 20.0,
            bottom_row_height: 32.0,
            chooser_row_height: 48.0,
            chooser_avatar: 32.0,
            notification_row_height: 56.0,
            notification_icon: 32.0,
            media_cover: 40.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WideLayout {
    pub card: Rect,
    pub card_radius: f32,
    pub content: CardContent,
    pub clock: WideClock,
    /// The quiet view's hint (lock screen, card hidden), centred.
    pub quiet_hint_bottom: f32,
    pub ramp: Ramp,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NarrowLayout {
    /// 0 is the quiet view, 1 the entry view; a swipe follows the finger.
    pub progress: f32,
    /// The clock's centre, from the top.
    pub clock_center_y: f32,
    pub inset: f32,
    pub time: TextStyle,
    pub date: TextStyle,
    /// 78% white.
    pub date_alpha: f32,
    /// The quiet view's hint, centred.
    pub quiet_hint_bottom: f32,
    /// Where the entry region (the lower 60%) starts, from the top.
    pub entry_top: f32,
    /// How far below its place the entry still is.
    pub entry_offset_y: f32,
    pub entry_opacity: f32,
    /// The scrim's strength, from none to full.
    pub scrim_strength: f32,
    pub status_row_top: f32,
    pub bottom_row_bottom: f32,
    pub content: CardContent,
    pub ramp: Ramp,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layout {
    Wide(WideLayout),
    Narrow(NarrowLayout),
}

/// What the layout depends on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutInput {
    pub width: f32,
    pub height: f32,
    pub large_text: bool,
    pub direction: Direction,
    /// The narrow layout's progress from quiet (0) to entry (1).
    pub entry_progress: f32,
}

/// The narrowest width that is laid out wide.
pub fn wide_threshold(large_text: bool) -> f32 {
    if large_text {
        CARD_WIDTH_LARGE_TEXT + CLOCK_MIN_WIDTH * LARGE_TEXT_SCALE
    } else {
        WIDE_MIN_WIDTH
    }
}

/// The narrow layout's time size, before the text scale.
pub fn narrow_time_size(width: f32) -> f32 {
    (width / 4.8).clamp(48.0, 96.0)
}

/// The scrim: a gradient from 45% black at the bottom start corner to clear
/// at the middle, so white text reads on any picture.
pub const SCRIM_ALPHA: f32 = 0.45;
/// The clock's shadow: 0 2 24 at 25% black.
pub const CLOCK_SHADOW_Y: f32 = 2.0;
pub const CLOCK_SHADOW_BLUR: f32 = 24.0;
pub const CLOCK_SHADOW_ALPHA: f32 = 0.25;
/// The power overlay's fill: 82% black.
pub const POWER_OVERLAY_ALPHA: f32 = 0.82;

/// Lays the screen out.
pub fn layout(input: LayoutInput) -> Layout {
    let scale = if input.large_text {
        LARGE_TEXT_SCALE
    } else {
        1.0
    };
    let ramp = Ramp::at(scale);
    if input.width >= wide_threshold(input.large_text) {
        let full = input.width >= FULL_CLOCK_MIN_WIDTH;
        let card_width = if input.large_text {
            CARD_WIDTH_LARGE_TEXT
        } else if full {
            CARD_WIDTH
        } else {
            CARD_WIDTH_COMPACT
        };
        let card_x = match input.direction.end() {
            Edge::Right => input.width - CARD_INSET - card_width,
            Edge::Left => CARD_INSET,
        };
        let card = Rect {
            x: card_x,
            y: CARD_INSET,
            width: card_width,
            height: (input.height - 2.0 * CARD_INSET).max(0.0),
        };
        let (weekday, time) = if full {
            (WEEKDAY, TIME)
        } else {
            (WEEKDAY_COMPACT, TIME_COMPACT)
        };
        Layout::Wide(WideLayout {
            card,
            card_radius: CARD_RADIUS,
            content: CardContent::new(&ramp, input.direction, 320.0),
            clock: WideClock {
                horizontal: Inset {
                    edge: input.direction.start(),
                    by: CLOCK_START_INSET,
                },
                bottom: CLOCK_BOTTOM_INSET,
                weekday: weekday.scaled(scale),
                row_gap: 16.0,
                time: time.scaled(scale),
                period: PERIOD.scaled(scale),
                period_gap: 8.0,
                period_alpha: 0.75,
                divider_width: 1.0,
                divider_height: 32.0,
                divider_gap: 16.0,
                divider_alpha: 0.35,
                date: DATE.scaled(scale),
                date_alpha: 0.90,
            },
            quiet_hint_bottom: QUIET_HINT_BOTTOM,
            ramp,
        })
    } else {
        let progress = input.entry_progress.clamp(0.0, 1.0);
        let center_fraction = 0.50 + (0.23 - 0.50) * progress;
        let time_size = narrow_time_size(input.width);
        // The full-size clock's ratio of line height to size, 64 / 56.
        let time = TextStyle::new(time_size, time_size * 64.0 / 56.0, 300).scaled(scale);
        let field_width = (input.width - 2.0 * NARROW_INSET).clamp(0.0, NARROW_FIELD_MAX_WIDTH);
        Layout::Narrow(NarrowLayout {
            progress,
            clock_center_y: input.height * center_fraction,
            inset: NARROW_INSET,
            time,
            date: NARROW_DATE.scaled(scale),
            date_alpha: 0.78,
            quiet_hint_bottom: QUIET_HINT_BOTTOM,
            entry_top: input.height * 0.40,
            entry_offset_y: motion::NARROW_ENTRY_RISE * (1.0 - progress),
            entry_opacity: progress,
            scrim_strength: progress,
            status_row_top: NARROW_INSET,
            bottom_row_bottom: NARROW_INSET,
            content: CardContent::new(&ramp, input.direction, field_width),
            ramp,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(width: f32) -> LayoutInput {
        LayoutInput {
            width,
            height: 900.0,
            large_text: false,
            direction: Direction::LeftToRight,
            entry_progress: 1.0,
        }
    }

    fn wide(input: LayoutInput) -> WideLayout {
        match layout(input) {
            Layout::Wide(wide) => wide,
            Layout::Narrow(_) => panic!("{} should be wide", input.width),
        }
    }

    fn narrow(input: LayoutInput) -> NarrowLayout {
        match layout(input) {
            Layout::Narrow(narrow) => narrow,
            Layout::Wide(_) => panic!("{} should be narrow", input.width),
        }
    }

    #[test]
    fn at_960_the_compact_wide_layout() {
        let layout = wide(LayoutInput {
            height: 600.0,
            ..input(960.0)
        });
        assert_eq!(
            layout.card,
            Rect {
                x: 500.0,
                y: 20.0,
                width: 440.0,
                height: 560.0
            }
        );
        assert_eq!(layout.clock.weekday.size, 88.0);
        assert_eq!(layout.clock.time.size, 44.0);
        assert_eq!(
            layout.clock.horizontal,
            Inset {
                edge: Edge::Left,
                by: 64.0
            }
        );
        assert_eq!(layout.clock.bottom, 56.0);
        assert_eq!(layout.content.status_row_edge, Edge::Right);
    }

    #[test]
    fn at_1280_1440_and_2560_the_full_wide_layout() {
        for (width, card_x) in [(1280.0, 780.0), (1440.0, 940.0), (2560.0, 2060.0)] {
            let layout = wide(input(width));
            assert_eq!(layout.card.width, 480.0, "{width}");
            assert_eq!(layout.card.x, card_x, "{width}");
            assert_eq!(layout.card.height, 860.0);
            assert_eq!(layout.card_radius, 8.0);
            assert_eq!(layout.clock.weekday, WEEKDAY);
            assert_eq!(layout.clock.weekday.size, 120.0);
            assert_eq!(layout.clock.time.size, 56.0);
            assert_eq!(layout.clock.period.size, 18.0);
            assert_eq!(layout.clock.date.size, 20.0);
            assert_eq!(layout.content.field_width, 320.0);
            assert_eq!(layout.content.avatar, 96.0);
            assert_eq!(layout.content.name.size, 28.0);
        }
    }

    #[test]
    fn the_thresholds_are_800_and_1100() {
        assert!(matches!(layout(input(800.0)), Layout::Wide(_)));
        assert!(matches!(layout(input(799.0)), Layout::Narrow(_)));
        assert_eq!(wide(input(1099.0)).card.width, 440.0);
        assert_eq!(wide(input(1100.0)).card.width, 480.0);
    }

    #[test]
    fn at_375_and_768_the_narrow_layout() {
        let phone = narrow(input(375.0));
        assert_eq!(phone.time.size, 375.0 / 4.8);
        assert_eq!(phone.content.field_width, 343.0);
        assert_eq!(phone.date.size, 17.0);
        assert_eq!(phone.inset, 16.0);
        let tablet = narrow(input(768.0));
        assert_eq!(tablet.time.size, 96.0);
        assert_eq!(tablet.content.field_width, 400.0);
        assert_eq!(narrow_time_size(200.0), 48.0);
    }

    #[test]
    fn the_narrow_clock_rises_as_the_entry_comes_in() {
        let quiet = narrow(LayoutInput {
            entry_progress: 0.0,
            ..input(768.0)
        });
        assert_eq!(quiet.clock_center_y, 450.0);
        assert_eq!(quiet.entry_offset_y, 48.0);
        assert_eq!(quiet.entry_opacity, 0.0);
        assert_eq!(quiet.scrim_strength, 0.0);
        let entry = narrow(input(768.0));
        assert!((entry.clock_center_y - 900.0 * 0.23).abs() < 1e-3);
        assert_eq!(entry.entry_offset_y, 0.0);
        assert_eq!(entry.entry_top, 360.0);
        let half = narrow(LayoutInput {
            entry_progress: 0.5,
            ..input(768.0)
        });
        assert!((half.clock_center_y - 900.0 * 0.365).abs() < 1e-3);
        assert_eq!(half.entry_offset_y, 24.0);
        let beyond = narrow(LayoutInput {
            entry_progress: 3.0,
            ..input(768.0)
        });
        assert_eq!(beyond.progress, 1.0);
    }

    #[test]
    fn large_text_scales_every_size_and_widens_the_card() {
        let layout = wide(LayoutInput {
            large_text: true,
            ..input(1280.0)
        });
        assert_eq!(layout.card.width, 560.0);
        assert_eq!(layout.clock.weekday.size, 150.0);
        assert_eq!(layout.clock.time.size, 70.0);
        assert_eq!(layout.ramp.body.size, 17.5);
        assert_eq!(layout.content.caption.size, 15.0);
        assert_eq!(wide_threshold(true), 1010.0);
        assert!(matches!(layout_at(1010.0, true), Layout::Wide(_)));
        let fallen_back = narrow(LayoutInput {
            large_text: true,
            ..input(960.0)
        });
        assert_eq!(fallen_back.time.size, 96.0 * 1.25);
        assert_eq!(fallen_back.ramp.title.size, 35.0);
    }

    fn layout_at(width: f32, large_text: bool) -> Layout {
        layout(LayoutInput {
            large_text,
            ..input(width)
        })
    }

    #[test]
    fn right_to_left_mirrors_the_card_and_the_clock() {
        let layout = wide(LayoutInput {
            direction: Direction::RightToLeft,
            ..input(1440.0)
        });
        assert_eq!(layout.card.x, 20.0);
        assert_eq!(
            layout.clock.horizontal,
            Inset {
                edge: Edge::Right,
                by: 64.0
            }
        );
        assert_eq!(layout.content.status_row_edge, Edge::Left);
        let narrow = narrow(LayoutInput {
            direction: Direction::RightToLeft,
            ..input(375.0)
        });
        assert_eq!(narrow.content.status_row_edge, Edge::Left);
    }
}

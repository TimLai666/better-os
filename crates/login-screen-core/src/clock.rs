//! The clock group's date and time (design/login-screen-spec.md sections 3.1,
//! 3.14, 3.15 and 5.7).
//!
//! Dates and times follow `LC_TIME` as GDM's do: the *names* (weekdays,
//! months, the morning and afternoon words) and the default hour cycle come
//! from the time locale, and the *order* of the parts comes from the screen's
//! language, the way GNOME takes a translated format string and fills it with
//! the `LC_TIME` locale's names. The owner's machine has `LANG=zh_TW.UTF-8`
//! and `LC_TIME=lzh_TW`.
//!
//! The names for `zh_TW` and `en_US` are Better OS's copy (the spec's
//! 「星期六」「下午 3:42」「11月21日」 and "Saturday", "3:42 PM", "November 21");
//! the `lzh_TW` ones are glibc's own (`/usr/share/i18n/locales/lzh_TW`, read
//! on 2026-11-22): weekdays 週日 to 週六, months 一月 to 十二月, 朝 and 暮,
//! and a 24-hour `t_fmt`. Figures are always ASCII digits, which the clock
//! draws tabular; `lzh_TW`'s `alt_digits` are not used.

use better_core::local_time::LocalTime;

use crate::copy::Language;

/// The locale the date's names come from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeLocale {
    ZhTw,
    EnUs,
    /// Literary Chinese, Taiwan.
    LzhTw,
}

impl TimeLocale {
    /// The time locale a POSIX locale name selects, or `None` for one Better
    /// OS has no names for.
    pub fn from_locale_name(locale: &str) -> Option<Self> {
        let base = locale
            .trim()
            .split(['.', '@'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match base.as_str() {
            "lzh_tw" => Some(Self::LzhTw),
            "zh_tw" => Some(Self::ZhTw),
            "en_us" | "en" => Some(Self::EnUs),
            _ if base.starts_with("en_") => Some(Self::EnUs),
            _ => None,
        }
    }

    /// The time locale from `locale1`'s `Locale` list (`LANG=…`, `LC_TIME=…`)
    /// or a process's environment, with POSIX's precedence: `LC_ALL`, then
    /// `LC_TIME`, then `LANG`. A locale with no names here falls back to the
    /// screen's language.
    pub fn resolve(assignments: &[(&str, &str)], language: Language) -> Self {
        let value = |name: &str| {
            assignments
                .iter()
                .find(|(key, value)| *key == name && !value.trim().is_empty())
                .map(|(_, value)| *value)
        };
        value("LC_ALL")
            .or_else(|| value("LC_TIME"))
            .or_else(|| value("LANG"))
            .and_then(Self::from_locale_name)
            .unwrap_or(match language {
                Language::ZhTw => Self::ZhTw,
                Language::EnUs => Self::EnUs,
            })
    }

    /// Splits `KEY=value` lines (`locale1`'s `Locale` property) for
    /// [`TimeLocale::resolve`].
    pub fn assignments(lines: &[String]) -> Vec<(&str, &str)> {
        lines
            .iter()
            .filter_map(|line| line.split_once('='))
            .collect()
    }

    /// The hour cycle the locale itself uses when nothing else says (the lock
    /// screen takes the person's `clock-format` instead). `zh_TW` and
    /// `en_US` are 12-hour as the spec draws them; `lzh_TW`'s `t_fmt` is
    /// 24-hour.
    pub fn default_hour_cycle(self) -> HourCycle {
        match self {
            Self::ZhTw | Self::EnUs => HourCycle::H12,
            Self::LzhTw => HourCycle::H24,
        }
    }

    /// Monday first, as `LocalTime::days_from_monday` counts.
    fn weekday(self, days_from_monday: u8) -> &'static str {
        let index = usize::from(days_from_monday.min(6));
        match self {
            Self::ZhTw => [
                "星期一",
                "星期二",
                "星期三",
                "星期四",
                "星期五",
                "星期六",
                "星期日",
            ][index],
            Self::EnUs => [
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
                "Sunday",
            ][index],
            Self::LzhTw => ["週一", "週二", "週三", "週四", "週五", "週六", "週日"][index],
        }
    }

    fn month(self, month: u8) -> &'static str {
        let index = usize::from(month.clamp(1, 12) - 1);
        match self {
            Self::ZhTw => [
                "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月",
                "12月",
            ][index],
            Self::EnUs => [
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ][index],
            Self::LzhTw => [
                "一月",
                "二月",
                "三月",
                "四月",
                "五月",
                "六月",
                "七月",
                "八月",
                "九月",
                "十月",
                "十一月",
                "十二月",
            ][index],
        }
    }

    fn period(self, afternoon: bool) -> &'static str {
        match (self, afternoon) {
            (Self::ZhTw, false) => "上午",
            (Self::ZhTw, true) => "下午",
            (Self::EnUs, false) => "AM",
            (Self::EnUs, true) => "PM",
            (Self::LzhTw, false) => "朝",
            (Self::LzhTw, true) => "暮",
        }
    }

    /// Where the period sits: 「下午 3:42」, "3:42 PM".
    fn period_side(self) -> PeriodSide {
        match self {
            Self::ZhTw | Self::LzhTw => PeriodSide::Before,
            Self::EnUs => PeriodSide::After,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HourCycle {
    /// 1 to 12 with a period.
    H12,
    /// 00 to 23, no period.
    H24,
}

impl HourCycle {
    /// GNOME's `org.gnome.desktop.interface clock-format` value.
    pub fn from_clock_format(value: &str) -> Option<Self> {
        match value.trim().trim_matches('\'') {
            "12h" => Some(Self::H12),
            "24h" => Some(Self::H24),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeriodSide {
    Before,
    After,
}

/// The period (AM/PM) of a 12-hour time and the side it is drawn on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Period {
    pub text: &'static str,
    pub side: PeriodSide,
}

/// Every text the clock group draws for one minute.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClockText {
    /// The wide layout's large first line: 「星期六」 / "Saturday".
    pub weekday: &'static str,
    /// The time's figures alone: `3:42`, or `15:42` on a 24-hour clock.
    pub time: String,
    /// Only on a 12-hour clock.
    pub period: Option<Period>,
    /// The wide layout's date: 「11月21日」 / "November 21".
    pub date: String,
    /// The narrow quiet view's line: 「11月21日 星期六」 / "Saturday,
    /// November 21".
    pub date_with_weekday: String,
    /// Time and period in one run, for a notification row: 「下午3:42」 /
    /// "3:42 PM".
    pub short_time: String,
    /// The clock group's accessible name: the date and time in full.
    pub spoken: String,
    /// The zone could not be read and this is UTC (ticket 146's rule).
    pub utc_fallback: bool,
}

/// Formats one wall-clock time.
pub fn format_clock(
    time: &LocalTime,
    language: Language,
    locale: TimeLocale,
    cycle: HourCycle,
) -> ClockText {
    let civil = time.civil;
    let weekday = locale.weekday(time.days_from_monday);
    let month = locale.month(civil.month);
    let (figures, period) = match cycle {
        HourCycle::H24 => (format!("{:02}:{:02}", civil.hour, civil.minute), None),
        HourCycle::H12 => {
            let hour = match civil.hour % 12 {
                0 => 12,
                hour => hour,
            };
            (
                format!("{hour}:{:02}", civil.minute),
                Some(Period {
                    text: locale.period(civil.hour >= 12),
                    side: locale.period_side(),
                }),
            )
        }
    };
    let short_time = match &period {
        None => figures.clone(),
        Some(Period {
            text,
            side: PeriodSide::Before,
        }) => match language {
            Language::ZhTw => format!("{text}{figures}"),
            Language::EnUs => format!("{text} {figures}"),
        },
        Some(Period {
            text,
            side: PeriodSide::After,
        }) => format!("{figures} {text}"),
    };
    let day = civil.day;
    let (date, date_with_weekday, spoken) = match language {
        Language::ZhTw => (
            format!("{month}{day}日"),
            format!("{month}{day}日 {weekday}"),
            format!("{month}{day}日{weekday} {short_time}"),
        ),
        Language::EnUs => (
            format!("{month} {day}"),
            format!("{weekday}, {month} {day}"),
            format!("{weekday}, {month} {day}, {short_time}"),
        ),
    };
    let utc = time.utc_fallback;
    let spoken = if utc { format!("{spoken} UTC") } else { spoken };
    ClockText {
        weekday,
        time: figures,
        period,
        date,
        date_with_weekday,
        short_time,
        spoken,
        utc_fallback: utc,
    }
}

#[cfg(test)]
mod tests {
    use better_core::local_time::TimeZone;

    use super::*;

    /// Saturday 21 November 2026, 15:42 in Taipei (UTC+8).
    const SATURDAY_AFTERNOON: i64 = 1_795_246_920;
    const TAIPEI: TimeZone = TimeZone::Fixed(8 * 3600);

    fn at(unix: i64) -> LocalTime {
        TAIPEI.at(unix)
    }

    #[test]
    fn the_fixture_is_the_saturday_the_mockups_draw() {
        let time = at(SATURDAY_AFTERNOON);
        assert_eq!((time.civil.month, time.civil.day), (11, 21));
        assert_eq!((time.civil.hour, time.civil.minute), (15, 42));
        assert_eq!(time.days_from_monday, 5);
    }

    #[test]
    fn zh_tw_reads_as_the_mockups() {
        let text = format_clock(
            &at(SATURDAY_AFTERNOON),
            Language::ZhTw,
            TimeLocale::ZhTw,
            HourCycle::H12,
        );
        assert_eq!(text.weekday, "星期六");
        assert_eq!(text.time, "3:42");
        assert_eq!(
            text.period,
            Some(Period {
                text: "下午",
                side: PeriodSide::Before
            })
        );
        assert_eq!(text.date, "11月21日");
        assert_eq!(text.date_with_weekday, "11月21日 星期六");
        assert_eq!(text.short_time, "下午3:42");
        assert_eq!(text.spoken, "11月21日星期六 下午3:42");
    }

    #[test]
    fn en_us_reads_as_the_spec() {
        let text = format_clock(
            &at(SATURDAY_AFTERNOON),
            Language::EnUs,
            TimeLocale::EnUs,
            HourCycle::H12,
        );
        assert_eq!(text.weekday, "Saturday");
        assert_eq!(text.time, "3:42");
        assert_eq!(text.period.unwrap().side, PeriodSide::After);
        assert_eq!(text.date, "November 21");
        assert_eq!(text.date_with_weekday, "Saturday, November 21");
        assert_eq!(text.short_time, "3:42 PM");
        assert_eq!(text.spoken, "Saturday, November 21, 3:42 PM");
    }

    #[test]
    fn the_owners_lzh_tw_time_locale_under_a_zh_tw_screen() {
        let assignments = [("LANG", "zh_TW.UTF-8"), ("LC_TIME", "lzh_TW")];
        let locale = TimeLocale::resolve(&assignments, Language::ZhTw);
        assert_eq!(locale, TimeLocale::LzhTw);
        let text = format_clock(
            &at(SATURDAY_AFTERNOON),
            Language::ZhTw,
            locale,
            locale.default_hour_cycle(),
        );
        assert_eq!(text.weekday, "週六");
        assert_eq!(text.time, "15:42");
        assert_eq!(text.period, None);
        assert_eq!(text.date, "十一月21日");
        assert_eq!(text.date_with_weekday, "十一月21日 週六");
        assert_eq!(text.spoken, "十一月21日週六 15:42");
        // With the person's 12-hour clock, lzh_TW's own period words.
        let twelve = format_clock(
            &at(SATURDAY_AFTERNOON),
            Language::ZhTw,
            locale,
            HourCycle::H12,
        );
        assert_eq!(twelve.short_time, "暮3:42");
    }

    #[test]
    fn midnight_noon_and_the_morning() {
        let midnight = SATURDAY_AFTERNOON - (15 * 3600 + 42 * 60);
        let text = format_clock(
            &at(midnight),
            Language::EnUs,
            TimeLocale::EnUs,
            HourCycle::H12,
        );
        assert_eq!(text.short_time, "12:00 AM");
        let noon = midnight + 12 * 3600;
        let text = format_clock(&at(noon), Language::ZhTw, TimeLocale::ZhTw, HourCycle::H12);
        assert_eq!(text.short_time, "下午12:00");
        let morning = midnight + 9 * 3600 + 5 * 60;
        let text = format_clock(
            &at(morning),
            Language::ZhTw,
            TimeLocale::ZhTw,
            HourCycle::H24,
        );
        assert_eq!(text.time, "09:05");
        assert_eq!(text.period, None);
    }

    #[test]
    fn every_weekday_and_month_has_a_name() {
        for locale in [TimeLocale::ZhTw, TimeLocale::EnUs, TimeLocale::LzhTw] {
            let days: std::collections::BTreeSet<_> = (0..7).map(|d| locale.weekday(d)).collect();
            assert_eq!(days.len(), 7);
            let months: std::collections::BTreeSet<_> = (1..=12).map(|m| locale.month(m)).collect();
            assert_eq!(months.len(), 12);
        }
    }

    #[test]
    fn the_time_locale_follows_posix_precedence() {
        let resolve = |pairs: &[(&str, &str)]| TimeLocale::resolve(pairs, Language::EnUs);
        assert_eq!(resolve(&[("LANG", "zh_TW.UTF-8")]), TimeLocale::ZhTw);
        assert_eq!(
            resolve(&[("LANG", "zh_TW.UTF-8"), ("LC_TIME", "en_US.UTF-8")]),
            TimeLocale::EnUs
        );
        assert_eq!(
            resolve(&[("LC_TIME", "en_US.UTF-8"), ("LC_ALL", "lzh_TW.UTF-8")]),
            TimeLocale::LzhTw
        );
        // A locale with no names here follows the screen's language.
        assert_eq!(resolve(&[("LC_TIME", "fr_FR.UTF-8")]), TimeLocale::EnUs);
        assert_eq!(
            TimeLocale::resolve(&[("LC_TIME", "")], Language::ZhTw),
            TimeLocale::ZhTw
        );
        let lines = vec!["LANG=zh_TW.UTF-8".to_string(), "LC_TIME=lzh_TW".to_string()];
        assert_eq!(
            TimeLocale::resolve(&TimeLocale::assignments(&lines), Language::ZhTw),
            TimeLocale::LzhTw
        );
    }

    #[test]
    fn gnome_clock_format_values() {
        assert_eq!(HourCycle::from_clock_format("'12h'"), Some(HourCycle::H12));
        assert_eq!(HourCycle::from_clock_format("24h"), Some(HourCycle::H24));
        assert_eq!(HourCycle::from_clock_format("auto"), None);
    }

    #[test]
    fn a_missing_zone_says_utc() {
        let mut time = at(SATURDAY_AFTERNOON);
        time.utc_fallback = true;
        let text = format_clock(&time, Language::EnUs, TimeLocale::EnUs, HourCycle::H12);
        assert!(text.utc_fallback);
        assert!(text.spoken.ends_with(" UTC"));
    }
}

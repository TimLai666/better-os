//! Brightness and the keyboard backlight.

/// Where the keyboard backlight is read and written. GNOME's settings daemon
/// is asked first; UPower's `KbdBacklight` is the fallback (spec 11.1).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyboardSource {
    /// `org.gnome.SettingsDaemon.Power.Keyboard`: `Brightness` is a
    /// percentage and `Steps` counts the positions, off included.
    Gnome { steps: u32 },
    /// `org.freedesktop.UPower.KbdBacklight`: a level from 0 to the maximum.
    UPower { max: u32 },
}

/// The keyboard backlight as the card shows it: a level from 0 (off) to
/// `levels`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyboardLevels {
    pub source: KeyboardSource,
    /// The highest level; also how many lit levels there are.
    pub levels: u32,
    pub level: u32,
}

impl KeyboardLevels {
    /// From GNOME's percentage and step count. `None` when there is no lit
    /// level to offer.
    pub fn from_gnome(percent: i32, steps: u32) -> Option<KeyboardLevels> {
        let levels = steps.checked_sub(1).filter(|l| *l > 0)?;
        let percent = percent.clamp(0, 100) as f64;
        let level = ((percent * levels as f64) / 100.0).round() as u32;
        Some(KeyboardLevels {
            source: KeyboardSource::Gnome { steps },
            levels,
            level: level.min(levels),
        })
    }

    /// From UPower's level and maximum.
    pub fn from_upower(level: i32, max: i32) -> Option<KeyboardLevels> {
        let levels = u32::try_from(max).ok().filter(|m| *m > 0)?;
        let level = u32::try_from(level.max(0)).unwrap_or(0).min(levels);
        Some(KeyboardLevels {
            source: KeyboardSource::UPower { max: levels },
            levels,
            level,
        })
    }

    /// What to write to the source for `level`.
    pub fn wire_value(&self, level: u32) -> i32 {
        let level = level.min(self.levels);
        match self.source {
            KeyboardSource::Gnome { .. } => {
                ((level as f64 * 100.0) / self.levels as f64).round() as i32
            }
            KeyboardSource::UPower { .. } => level as i32,
        }
    }
}

/// The 0–100 the brightness slider shows; GNOME reports -1 when it has no
/// backlight it can drive, which is "not there", not zero.
pub fn brightness(raw: i32) -> Option<u8> {
    (0..=100).contains(&raw).then_some(raw as u8)
}

/// How often brightness is written while the slider is dragged.
pub const BRIGHTNESS_INTERVAL_MS: u64 = 50;

/// Spaces writes at least `interval` apart and keeps the last value a drag
/// produced, so the final position is always written.
#[derive(Clone, Debug)]
pub struct Throttle {
    interval_ms: u64,
    last_sent_ms: Option<u64>,
    pending: Option<u8>,
}

impl Default for Throttle {
    /// Brightness's: at most every [`BRIGHTNESS_INTERVAL_MS`].
    fn default() -> Self {
        Throttle::new(BRIGHTNESS_INTERVAL_MS)
    }
}

impl Throttle {
    pub fn new(interval_ms: u64) -> Throttle {
        Throttle {
            interval_ms,
            last_sent_ms: None,
            pending: None,
        }
    }

    /// A new slider value at `now_ms`. Returns the value to write now, if
    /// the interval has passed; otherwise it is kept for [`Throttle::due`].
    pub fn push(&mut self, now_ms: u64, value: u8) -> Option<u8> {
        if self.ready(now_ms) {
            self.pending = None;
            self.last_sent_ms = Some(now_ms);
            Some(value)
        } else {
            self.pending = Some(value);
            None
        }
    }

    /// The kept value, once its time has come.
    pub fn due(&mut self, now_ms: u64) -> Option<u8> {
        if self.pending.is_some() && self.ready(now_ms) {
            self.last_sent_ms = Some(now_ms);
            self.pending.take()
        } else {
            None
        }
    }

    /// How long until the kept value is due, when there is one.
    pub fn wait_ms(&self, now_ms: u64) -> Option<u64> {
        self.pending?;
        let last = self.last_sent_ms?;
        Some((last + self.interval_ms).saturating_sub(now_ms))
    }

    fn ready(&self, now_ms: u64) -> bool {
        self.last_sent_ms
            .is_none_or(|last| now_ms.saturating_sub(last) >= self.interval_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_steps_four_is_three_levels_and_off() {
        // gsd-power on the owner's machine: Steps 4.
        let k = KeyboardLevels::from_gnome(67, 4).unwrap();
        assert_eq!((k.levels, k.level), (3, 2));
        assert_eq!(k.wire_value(0), 0);
        assert_eq!(k.wire_value(1), 33);
        assert_eq!(k.wire_value(3), 100);
        assert_eq!(k.wire_value(9), 100);
        assert_eq!(KeyboardLevels::from_gnome(0, 4).unwrap().level, 0);
        assert_eq!(KeyboardLevels::from_gnome(100, 4).unwrap().level, 3);
    }

    #[test]
    fn upower_levels_are_written_as_they_are() {
        let k = KeyboardLevels::from_upower(1, 3).unwrap();
        assert_eq!((k.levels, k.level), (3, 1));
        assert_eq!(k.wire_value(2), 2);
        assert_eq!(KeyboardLevels::from_upower(7, 3).unwrap().level, 3);
    }

    #[test]
    fn a_backlight_with_no_lit_level_is_not_offered() {
        assert_eq!(KeyboardLevels::from_gnome(0, 1), None);
        assert_eq!(KeyboardLevels::from_gnome(0, 0), None);
        assert_eq!(KeyboardLevels::from_upower(0, 0), None);
    }

    #[test]
    fn brightness_minus_one_is_not_a_value() {
        assert_eq!(brightness(-1), None);
        assert_eq!(brightness(0), Some(0));
        assert_eq!(brightness(100), Some(100));
        assert_eq!(brightness(101), None);
    }

    #[test]
    fn a_drag_writes_at_most_every_fifty_ms_and_always_writes_the_last_value() {
        let mut t = Throttle::default();
        assert_eq!(t.push(0, 10), Some(10));
        assert_eq!(t.push(10, 11), None);
        assert_eq!(t.push(30, 12), None);
        assert_eq!(t.wait_ms(30), Some(20));
        assert_eq!(t.due(40), None);
        assert_eq!(t.due(50), Some(12));
        assert_eq!(t.due(51), None);
        assert_eq!(t.push(60, 13), None);
        assert_eq!(t.push(100, 14), Some(14));
        assert_eq!(t.due(500), None);
        assert_eq!(t.wait_ms(500), None);
    }
}

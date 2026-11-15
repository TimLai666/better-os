//! The two factors, their bounds, and where the service keeps them.
//!
//! The bounds are `touchpad_core::ScrollFactor`'s, so the window, the
//! configuration file, and the service refuse the same values.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use touchpad_core::ScrollFactor;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Factors {
    pub vertical: f64,
    pub horizontal: f64,
}

impl Factors {
    pub const NEUTRAL: Self = Self {
        vertical: 1.0,
        horizontal: 1.0,
    };

    /// Both factors, refused unless each is a valid `ScrollFactor`.
    pub fn new(vertical: f64, horizontal: f64) -> Result<Self, String> {
        let vertical = ScrollFactor::new(vertical).map_err(|error| error.to_string())?;
        let horizontal = ScrollFactor::new(horizontal).map_err(|error| error.to_string())?;
        Ok(Self {
            vertical: vertical.get(),
            horizontal: horizontal.get(),
        })
    }

    /// Neutral means the pass-through holds nothing: no grab, no clone.
    pub fn is_neutral(&self) -> bool {
        self.vertical == 1.0 && self.horizontal == 1.0
    }

    /// The file format: two lines, nothing else.
    pub fn to_file(self) -> String {
        format!(
            "vertical={}\nhorizontal={}\n",
            self.vertical, self.horizontal
        )
    }

    /// Anything but exactly the two lines `to_file` writes, with valid values,
    /// is refused.
    pub fn from_file(text: &str) -> Result<Self, String> {
        let mut vertical = None;
        let mut horizontal = None;
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("not a key=value line: {line:?}"))?;
            let value: f64 = value
                .trim()
                .parse()
                .map_err(|_| format!("not a number: {value:?}"))?;
            let slot = match key.trim() {
                "vertical" => &mut vertical,
                "horizontal" => &mut horizontal,
                other => return Err(format!("unknown key {other:?}")),
            };
            if slot.replace(value).is_some() {
                return Err(format!("{key} is given twice"));
            }
        }
        match (vertical, horizontal) {
            (Some(vertical), Some(horizontal)) => Self::new(vertical, horizontal),
            _ => Err("both factors must be given".to_string()),
        }
    }

    /// The stored factors, or neutral when there are none or they are not
    /// valid. A bad file must never make the touchpad move faster than the
    /// person chose, so the answer to "unreadable" is "held nothing".
    pub fn load(path: &Path) -> (Self, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => match Self::from_file(&text) {
                Ok(factors) => (factors, None),
                Err(error) => (Self::NEUTRAL, Some(format!("{}: {error}", path.display()))),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Self::NEUTRAL, None),
            Err(error) => (Self::NEUTRAL, Some(format!("{}: {error}", path.display()))),
        }
    }

    /// Written to a temporary file and renamed, so a crash mid-write leaves
    /// the previous factors. The temporary name carries the process id: every
    /// touchpad's service writes the same file.
    pub fn store(self, path: &Path) -> std::io::Result<()> {
        let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
        std::fs::write(&temporary, self.to_file())?;
        std::fs::rename(&temporary, path)
    }
}

/// The factors as the bus thread writes them and the event loop reads them,
/// once per frame, without a lock.
#[derive(Clone, Debug)]
pub struct SharedFactors {
    vertical: Arc<AtomicU64>,
    horizontal: Arc<AtomicU64>,
}

impl SharedFactors {
    pub fn new(factors: Factors) -> Self {
        Self {
            vertical: Arc::new(AtomicU64::new(factors.vertical.to_bits())),
            horizontal: Arc::new(AtomicU64::new(factors.horizontal.to_bits())),
        }
    }

    pub fn get(&self) -> Factors {
        Factors {
            vertical: f64::from_bits(self.vertical.load(Ordering::Acquire)),
            horizontal: f64::from_bits(self.horizontal.load(Ordering::Acquire)),
        }
    }

    pub fn set(&self, factors: Factors) {
        self.vertical
            .store(factors.vertical.to_bits(), Ordering::Release);
        self.horizontal
            .store(factors.horizontal.to_bits(), Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bounds_are_the_settings_windows_bounds() {
        assert!(Factors::new(ScrollFactor::MIN, ScrollFactor::MAX).is_ok());
        assert!(Factors::new(ScrollFactor::MIN - 0.01, 1.0).is_err());
        assert!(Factors::new(1.0, ScrollFactor::MAX + 0.01).is_err());
        assert!(Factors::new(f64::NAN, 1.0).is_err());
        assert!(Factors::new(1.0, f64::INFINITY).is_err());
    }

    #[test]
    fn the_file_round_trips_and_refuses_anything_else() {
        let factors = Factors::new(2.5, 0.5).unwrap();
        assert_eq!(Factors::from_file(&factors.to_file()), Ok(factors));
        for bad in [
            "",
            "vertical=2\n",
            "vertical=2\nhorizontal=1\nvertical=3\n",
            "vertical=2\nhorizontal=9\n",
            "vertical=2\nhorizontal=1\nspeed=4\n",
            "vertical 2\nhorizontal=1\n",
        ] {
            assert!(Factors::from_file(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_missing_or_bad_file_holds_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("factors");
        assert_eq!(Factors::load(&path), (Factors::NEUTRAL, None));
        std::fs::write(&path, "vertical=50\nhorizontal=1\n").unwrap();
        let (factors, error) = Factors::load(&path);
        assert!(factors.is_neutral());
        assert!(error.is_some());
        Factors::new(3.0, 1.0).unwrap().store(&path).unwrap();
        assert_eq!(Factors::load(&path).0, Factors::new(3.0, 1.0).unwrap());
    }

    #[test]
    fn shared_factors_carry_the_last_value_set() {
        let shared = SharedFactors::new(Factors::NEUTRAL);
        let reader = shared.clone();
        shared.set(Factors::new(1.5, 0.75).unwrap());
        assert_eq!(reader.get(), Factors::new(1.5, 0.75).unwrap());
    }
}

//! Motion inside a fixed layer surface. A panel that slides in or follows the
//! fingers is not moved by the compositor: GPUI cannot change a layer
//! surface's margin after creation, and recreating it every frame would
//! flicker. The surface stays where it is, at its full size, and the panel is
//! drawn at an offset inside it, with the input region trimmed to the part
//! that is drawn ([`visible_part`]).
//!
//! Progress runs from 0 (hidden) to 1 (shown). Time is a [`Duration`] since
//! any fixed instant the caller picks, so every rule is a pure function of
//! the time it is asked about. The settle constants are here and nowhere
//! else (engineering/specs/better-os-session.md, "For the implementer").

use std::time::Duration;

use super::rules::{Edges, Rect};

/// How long a slide over the whole distance takes.
pub const SLIDE: Duration = Duration::from_millis(220);

/// The shortest settle after the fingers lift, however little is left.
pub const SETTLE_MIN: Duration = Duration::from_millis(90);

/// Progress per second above which a lift continues in the fingers'
/// direction rather than going to the nearer end.
pub const FLICK_VELOCITY: f32 = 1.5;

/// Ease out: fast at first, slowing into the end, as GNOME's and COSMIC's
/// own panels move.
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Still(f32),
    Moving {
        from: f32,
        to: f32,
        start: Duration,
        length: Duration,
    },
    Following(f32),
}

/// The progress of one panel, from 0 (hidden) to 1 (shown).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    state: State,
}

impl Motion {
    /// At rest at `progress`.
    pub fn at(progress: f32) -> Motion {
        Motion {
            state: State::Still(clamp(progress)),
        }
    }

    /// The progress at `now`.
    pub fn value(&self, now: Duration) -> f32 {
        match self.state {
            State::Still(value) | State::Following(value) => value,
            State::Moving {
                from,
                to,
                start,
                length,
            } => {
                if length.is_zero() || now >= start + length {
                    return to;
                }
                let elapsed = now.saturating_sub(start).as_secs_f32();
                from + (to - from) * ease_out_cubic(elapsed / length.as_secs_f32())
            }
        }
    }

    /// The end it is at or moving to; while following, the nearer end.
    pub fn target(&self) -> f32 {
        match self.state {
            State::Still(value) => value,
            State::Moving { to, .. } => to,
            State::Following(value) => value.round(),
        }
    }

    /// Whether it needs no more frames after `now`.
    pub fn is_settled(&self, now: Duration) -> bool {
        match self.state {
            State::Still(_) => true,
            State::Following(_) => false,
            State::Moving { start, length, .. } => now >= start + length,
        }
    }

    /// Starts moving from wherever it is at `now` to `to`, taking the share of
    /// [`SLIDE`] the distance is, so a reversed half slide is half as long.
    pub fn slide_to(&mut self, to: f32, now: Duration) {
        let from = self.value(now);
        let to = clamp(to);
        let length = SLIDE.mul_f32((to - from).abs());
        self.state = if length.is_zero() {
            State::Still(to)
        } else {
            State::Moving {
                from,
                to,
                start: now,
                length,
            }
        };
    }

    /// Puts it where the fingers are: no easing, no delay.
    pub fn follow(&mut self, progress: f32) {
        self.state = State::Following(clamp(progress));
    }

    /// The fingers lifted at `now`, moving at `velocity` progress per second:
    /// a flick goes on in its direction, anything slower goes to the nearer
    /// end. Returns the end it settles to.
    pub fn release(&mut self, velocity: f32, now: Duration) -> f32 {
        let at = self.value(now);
        let to = if velocity.is_finite() && velocity.abs() >= FLICK_VELOCITY {
            if velocity > 0.0 { 1.0 } else { 0.0 }
        } else if at >= 0.5 {
            1.0
        } else {
            0.0
        };
        let length = SLIDE.mul_f32((to - at).abs()).max(SETTLE_MIN);
        self.state = State::Moving {
            from: at,
            to,
            start: now,
            length,
        };
        to
    }
}

fn clamp(progress: f32) -> f32 {
    if progress.is_finite() {
        progress.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The part of a surface `size` big that a panel sliding in from `edge`
/// covers at `progress`: the panel is the whole surface, pushed off the edge
/// by the share not yet shown. This is both where to draw it and the input
/// region; nothing is drawn, and nothing takes input, at 0.
pub fn visible_part(size: (f32, f32), edge: Edges, progress: f32) -> Option<Rect> {
    let (width, height) = size;
    let progress = clamp(progress);
    if progress <= 0.0 || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let rect = if edge == Edges::TOP {
        Rect::new(0.0, 0.0, width, height * progress)
    } else if edge == Edges::BOTTOM {
        let shown = height * progress;
        Rect::new(0.0, height - shown, width, shown)
    } else if edge == Edges::LEFT {
        Rect::new(0.0, 0.0, width * progress, height)
    } else if edge == Edges::RIGHT {
        let shown = width * progress;
        Rect::new(width - shown, 0.0, shown, height)
    } else {
        // Not from an edge: a fade or a scale; all of the surface is drawn.
        Rect::new(0.0, 0.0, width, height)
    };
    Some(rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn a_slide_eases_out_and_ends_where_it_was_sent() {
        let mut motion = Motion::at(0.0);
        motion.slide_to(1.0, ms(1000));
        assert_eq!(motion.value(ms(1000)), 0.0);
        let half = motion.value(ms(1000) + SLIDE / 2);
        assert!(half > 0.5, "eased out, past halfway at half time: {half}");
        assert!(!motion.is_settled(ms(1100)));
        assert_eq!(motion.value(ms(1000) + SLIDE), 1.0);
        assert!(motion.is_settled(ms(1000) + SLIDE));
        assert_eq!(motion.target(), 1.0);
    }

    #[test]
    fn a_reversed_slide_starts_where_it_is_and_takes_its_share_of_the_time() {
        let mut motion = Motion::at(0.0);
        motion.slide_to(1.0, ms(0));
        let at = motion.value(ms(50));
        motion.slide_to(0.0, ms(50));
        assert_eq!(motion.value(ms(50)), at, "no jump on reversal");
        let length = SLIDE.mul_f32(at);
        assert_eq!(motion.value(ms(50) + length), 0.0);
        assert!(motion.is_settled(ms(50) + length));
    }

    #[test]
    fn following_has_no_easing_and_is_never_settled() {
        let mut motion = Motion::at(0.0);
        motion.follow(0.3);
        assert_eq!(motion.value(ms(0)), 0.3);
        assert_eq!(motion.value(ms(10_000)), 0.3);
        assert!(!motion.is_settled(ms(10_000)));
        motion.follow(1.7);
        assert_eq!(motion.value(ms(0)), 1.0, "clamped");
        motion.follow(f32::NAN);
        assert_eq!(motion.value(ms(0)), 0.0);
    }

    #[test]
    fn a_slow_release_goes_to_the_nearer_end() {
        let mut motion = Motion::at(0.0);
        motion.follow(0.6);
        assert_eq!(motion.release(0.2, ms(500)), 1.0);
        assert_eq!(motion.value(ms(500)), 0.6, "settles from where it was");
        let mut motion = Motion::at(0.0);
        motion.follow(0.4);
        assert_eq!(motion.release(-0.2, ms(500)), 0.0);
    }

    #[test]
    fn a_flick_goes_on_in_its_direction() {
        let mut motion = Motion::at(0.0);
        motion.follow(0.2);
        assert_eq!(motion.release(FLICK_VELOCITY, ms(0)), 1.0);
        let mut motion = Motion::at(1.0);
        motion.follow(0.9);
        assert_eq!(motion.release(-3.0, ms(0)), 0.0, "turned back near the end");
    }

    #[test]
    fn a_settle_takes_at_least_the_minimum() {
        let mut motion = Motion::at(0.0);
        motion.follow(0.99);
        motion.release(0.0, ms(0));
        assert!(!motion.is_settled(SETTLE_MIN - ms(1)));
        assert!(motion.is_settled(SETTLE_MIN));
        assert_eq!(motion.value(SETTLE_MIN), 1.0);
    }

    #[test]
    fn the_visible_part_grows_from_the_edge() {
        let size = (1280.0, 400.0);
        assert_eq!(visible_part(size, Edges::TOP, 0.0), None);
        assert_eq!(
            visible_part(size, Edges::TOP, 0.25),
            Some(Rect::new(0.0, 0.0, 1280.0, 100.0))
        );
        assert_eq!(
            visible_part(size, Edges::BOTTOM, 0.25),
            Some(Rect::new(0.0, 300.0, 1280.0, 100.0))
        );
        assert_eq!(
            visible_part(size, Edges::RIGHT, 0.5),
            Some(Rect::new(640.0, 0.0, 640.0, 400.0))
        );
        assert_eq!(
            visible_part(size, Edges::LEFT, 1.0),
            Some(Rect::new(0.0, 0.0, 1280.0, 400.0))
        );
        assert_eq!(
            visible_part(size, Edges::NONE, 0.1),
            Some(Rect::new(0.0, 0.0, 1280.0, 400.0))
        );
        assert_eq!(visible_part((0.0, 10.0), Edges::TOP, 1.0), None);
    }

    #[test]
    fn the_curve_is_bounded() {
        assert_eq!(ease_out_cubic(-1.0), 0.0);
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert_eq!(ease_out_cubic(1.0), 1.0);
        assert_eq!(ease_out_cubic(2.0), 1.0);
    }
}

//! Rectangles and points in the desktop's logical pixels, the space every
//! output's position and size is given in (`xdg_output`), and the space
//! GNOME's stage coordinates are in.

use serde::{Deserialize, Serialize};

/// A point in logical pixels. Pointer positions are fractional.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Point {
        Point { x, y }
    }
}

/// A rectangle of whole logical pixels.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> i32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    /// Whether the point lies inside, the left and top edges included.
    pub fn contains(&self, point: Point) -> bool {
        point.x >= f64::from(self.x)
            && point.y >= f64::from(self.y)
            && point.x < f64::from(self.right())
            && point.y < f64::from(self.bottom())
    }

    /// Whether `other` lies wholly inside this one.
    pub fn contains_rect(&self, other: &Rect) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }

    /// The part both share, or `None` when they do not meet.
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > x && bottom > y).then(|| Rect::new(x, y, right - x, bottom - y))
    }

    /// The smallest rectangle holding both.
    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Rect::new(x, y, right - x, bottom - y)
    }

    /// The same rectangle moved so `origin` is its top left corner's new
    /// origin of coordinates.
    pub fn relative_to(&self, origin_x: i32, origin_y: i32) -> Rect {
        Rect::new(self.x - origin_x, self.y - origin_y, self.width, self.height)
    }

    /// The centre, for choosing between rectangles by distance.
    pub fn center(&self) -> Point {
        Point::new(
            f64::from(self.x) + f64::from(self.width) / 2.0,
            f64::from(self.y) + f64::from(self.height) / 2.0,
        )
    }
}

/// The bounding box of every output: GNOME's stage. `None` with no outputs.
pub fn stage_of<'a>(outputs: impl IntoIterator<Item = &'a Rect>) -> Option<Rect> {
    outputs.into_iter().copied().reduce(|a, b| a.union(&b))
}

#[cfg(test)]
mod tests {
    use super::{Point, Rect, stage_of};

    #[test]
    fn intersection_union_and_containment() {
        let a = Rect::new(0, 0, 100, 50);
        let b = Rect::new(50, 25, 100, 100);
        assert_eq!(a.intersection(&b), Some(Rect::new(50, 25, 50, 25)));
        assert_eq!(a.union(&b), Rect::new(0, 0, 150, 125));
        assert_eq!(a.intersection(&Rect::new(100, 0, 10, 10)), None);
        assert!(a.contains(Point::new(0.0, 0.0)));
        assert!(!a.contains(Point::new(100.0, 10.0)));
        assert!(a.contains_rect(&Rect::new(10, 10, 90, 40)));
        assert!(!a.contains_rect(&Rect::new(10, 10, 91, 40)));
    }

    #[test]
    fn the_stage_is_every_output_together() {
        let outputs = [Rect::new(0, 0, 1280, 800), Rect::new(1280, 0, 1920, 1080)];
        assert_eq!(stage_of(&outputs), Some(Rect::new(0, 0, 3200, 1080)));
        assert_eq!(stage_of(&[]), None);
    }
}

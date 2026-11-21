//! The area selector: GNOME Shell 46's `UIAreaSelector` (`js/ui/screenshot.js`
//! 233–708), rule for rule, in the stage's logical pixels.
//!
//! The area is two corners, `start` and `last`, each a whole pixel inside the
//! stage; the area is every pixel between them, both included, so a press
//! and release on one pixel is a 1 × 1 area before the click rule enlarges
//! it. While a drag moves or resizes, `start` is the side that stays and
//! `last` the side that moves, so dragging a handle past the opposite side
//! turns the area over without any special case.

use crate::geometry::{Point, Rect};

/// What a press at a point would do, and so which pointer shape shows there.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Grip {
    /// Outside the area: a new area is drawn from here.
    Crosshair,
    /// Inside the area: it moves.
    Move,
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

impl Grip {
    fn moves_left_side(self) -> bool {
        matches!(self, Grip::NorthWest | Grip::West | Grip::SouthWest)
    }
    fn moves_right_side(self) -> bool {
        matches!(self, Grip::NorthEast | Grip::East | Grip::SouthEast)
    }
    fn moves_top_side(self) -> bool {
        matches!(self, Grip::NorthWest | Grip::North | Grip::NorthEast)
    }
    fn moves_bottom_side(self) -> bool {
        matches!(self, Grip::SouthWest | Grip::South | Grip::SouthEast)
    }
}

/// Which button began a drag. GNOME takes the primary and secondary
/// buttons and touch; GPUI gives no touch, so two are enough.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Button {
    Primary,
    Secondary,
}

/// GNOME's handle size: `$medium_icon_size` (`_screenshot.scss`
/// `.screenshot-ui-area-selector-handle`), in logical pixels.
pub const HANDLE_SIZE: f64 = 24.0;
/// How far outside an edge still grabs it, before the scale (`10 *
/// scaleFactor`, 351).
pub const EDGE_THRESHOLD: f64 = 10.0;
/// How far a click without a drag grows the area each way (`20 *
/// scaleFactor`, 396).
pub const CLICK_GROWTH: f64 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Drag {
    button: Button,
    grip: Grip,
    /// Where the pointer was, adjusted for rubber banding, at the last
    /// motion.
    from: Point,
}

/// The area and the drag in progress, if any.
#[derive(Clone, Debug, PartialEq)]
pub struct AreaSelector {
    start: (i32, i32),
    last: (i32, i32),
    drag: Option<Drag>,
    /// The interface's scale factor (GNOME's `St.ThemeContext.scaleFactor`),
    /// which grows the edge threshold and the click growth.
    scale: f64,
}

/// What [`AreaSelector::release`] reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DragEnd {
    /// A drag ended; the panel comes back to full opacity.
    Ended,
    /// The release was not the drag's button: nothing happened.
    Ignored,
}

impl AreaSelector {
    /// An area that fits no stage, so the first [`AreaSelector::reset`] puts
    /// it in the middle of the primary output (GNOME's `_startX = -1`, 276).
    pub fn new(scale: f64) -> AreaSelector {
        AreaSelector {
            start: (i32::MIN / 2, 0),
            last: (0, 0),
            drag: None,
            scale,
        }
    }

    /// A selector holding a remembered area. It is checked against the stage
    /// by [`AreaSelector::reset`] like any other.
    pub fn remembered(area: Rect, scale: f64) -> AreaSelector {
        AreaSelector {
            start: (area.x, area.y),
            last: (area.x + area.width - 1, area.y + area.height - 1),
            drag: None,
            scale,
        }
    }

    pub fn set_scale(&mut self, scale: f64) {
        self.scale = scale;
    }

    /// The area: the pixels between the two corners, both included.
    pub fn geometry(&self) -> Rect {
        let left = self.start.0.min(self.last.0);
        let top = self.start.1.min(self.last.1);
        let right = self.start.0.max(self.last.0);
        let bottom = self.start.1.max(self.last.1);
        Rect::new(left, top, right - left + 1, bottom - top + 1)
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The grip of the drag in progress, which follows a handle dragged past
    /// the opposite side.
    pub fn drag_grip(&self) -> Option<Grip> {
        self.drag.map(|drag| drag.grip)
    }

    /// Keeps the area when it still fits the stage and puts it back in the
    /// middle quarter of `primary` (from 3/8 to 5/8 of each side) when it
    /// does not, as GNOME does each time the interface closes (`reset`
    /// 284–312). With no primary output the area is left out of bounds, so
    /// the next reset with an output fixes it.
    pub fn reset(&mut self, stage: Option<Rect>, primary: Option<Rect>) {
        self.drag = None;
        let area = self.geometry();
        let fits = stage.is_some_and(|stage| stage.contains_rect(&area));
        if fits {
            return;
        }
        self.start = (i32::MIN / 2, 0);
        self.last = (0, 0);
        if let Some(monitor) = primary {
            self.start = (
                monitor.x + monitor.width * 3 / 8,
                monitor.y + monitor.height * 3 / 8,
            );
            self.last = (
                monitor.x + monitor.width * 5 / 8 - 1,
                monitor.y + monitor.height * 5 / 8 - 1,
            );
        }
    }

    /// What a press at `point` would grab (`_computeCursorType` 334–378):
    /// the round handles first, then a band [`EDGE_THRESHOLD`] wide outside
    /// each edge and its corner squares, then the inside.
    pub fn grip_at(&self, point: Point) -> Grip {
        let area = self.geometry();
        let left = f64::from(area.x);
        let top = f64::from(area.y);
        let right = f64::from(area.x + area.width - 1);
        let bottom = f64::from(area.y + area.height - 1);
        let (x, y) = (point.x, point.y);

        let limit = (HANDLE_SIZE / 2.0).powi(2);
        let near = |cx: f64, cy: f64| (cx - x).powi(2) + (cy - y).powi(2) <= limit;
        if near(left, top) {
            return Grip::NorthWest;
        } else if near(right, top) {
            return Grip::NorthEast;
        } else if near(left, bottom) {
            return Grip::SouthWest;
        } else if near(right, bottom) {
            return Grip::SouthEast;
        }

        let threshold = EDGE_THRESHOLD * self.scale;
        let above = top - y >= 0.0 && top - y <= threshold;
        let below = y - bottom >= 0.0 && y - bottom <= threshold;
        let between_rows = top - y < 0.0 && y - bottom < 0.0;
        if left - x >= 0.0 && left - x <= threshold {
            if above {
                return Grip::NorthWest;
            } else if below {
                return Grip::SouthWest;
            } else if between_rows {
                return Grip::West;
            }
        } else if x - right >= 0.0 && x - right <= threshold {
            if above {
                return Grip::NorthEast;
            } else if below {
                return Grip::SouthEast;
            } else if between_rows {
                return Grip::East;
            }
        } else if left - x < 0.0 && x - right < 0.0 {
            if above {
                return Grip::North;
            } else if below {
                return Grip::South;
            } else if between_rows {
                return Grip::Move;
            }
        }
        Grip::Crosshair
    }

    /// A press (`_onPress` 431–511). Outside the area, with the secondary
    /// button, or with Ctrl it starts a new area at the pixel under the
    /// pointer; elsewhere it starts moving or resizing. Returns false when a
    /// drag is already in progress, which the press then does not change.
    pub fn press(&mut self, point: Point, button: Button, ctrl: bool) -> bool {
        if self.drag.is_some() {
            return false;
        }
        let grip = self.grip_at(point);
        if grip == Grip::Crosshair || button == Button::Secondary || ctrl {
            let pixel = (point.x.floor() as i32, point.y.floor() as i32);
            self.start = pixel;
            self.last = pixel;
            self.drag = Some(Drag {
                button,
                grip: Grip::Crosshair,
                from: point,
            });
            return true;
        }

        let area = self.geometry();
        let (left, top) = (area.x, area.y);
        let right = left + area.width - 1;
        let bottom = top + area.height - 1;
        if grip == Grip::Move {
            self.start = (left, top);
            self.last = (right, bottom);
        }
        if grip.moves_left_side() {
            self.start.0 = right;
            self.last.0 = left;
        }
        if grip.moves_right_side() {
            self.start.0 = left;
            self.last.0 = right;
        }
        if grip.moves_top_side() {
            self.start.1 = bottom;
            self.last.1 = top;
        }
        if grip.moves_bottom_side() {
            self.start.1 = top;
            self.last.1 = bottom;
        }
        self.drag = Some(Drag {
            button,
            grip,
            from: point,
        });
        true
    }

    /// The pointer moved (`_onMotion` 528–663). Returns the pointer shape to
    /// show: the grip under the pointer without a drag, the drag's own grip
    /// during one.
    pub fn motion(&mut self, point: Point, stage: Rect) -> Grip {
        let Some(mut drag) = self.drag else {
            return self.grip_at(point);
        };
        let (min_x, min_y) = (stage.x, stage.y);
        let (max_x, max_y) = (stage.right() - 1, stage.bottom() - 1);

        if drag.grip == Grip::Crosshair {
            self.last = (point.x.floor() as i32, point.y.floor() as i32);
            self.last.0 = self.last.0.clamp(min_x, max_x);
            self.last.1 = self.last.1.clamp(min_y, max_y);
            self.drag = Some(drag);
            return Grip::Crosshair;
        }

        let mut dx = (point.x - drag.from.x).round() as i32;
        let mut dy = (point.y - drag.from.y).round() as i32;

        if drag.grip == Grip::Move {
            let area = self.geometry();
            let mut start_x = self.start.0 + dx;
            let mut start_y = self.start.1 + dy;
            let mut last_x = self.last.0 + dx;
            let mut last_y = self.last.1 + dy;
            let mut overshoot_x = 0;
            let mut overshoot_y = 0;
            // The size stays when the area meets the stage's edge.
            if start_x < min_x {
                overshoot_x = min_x - start_x;
                start_x = min_x;
                last_x = start_x + (area.width - 1);
            } else if last_x > max_x {
                overshoot_x = max_x - last_x;
                last_x = max_x;
                start_x = last_x - (area.width - 1);
            }
            if start_y < min_y {
                overshoot_y = min_y - start_y;
                start_y = min_y;
                last_y = start_y + (area.height - 1);
            } else if last_y > max_y {
                overshoot_y = max_y - last_y;
                last_y = max_y;
                start_y = last_y - (area.height - 1);
            }
            // The overshoot goes into the delta: the pointer slides over the
            // area while it is held at the edge (rubber banding).
            dx += overshoot_x;
            dy += overshoot_y;
            self.start = (start_x, start_y);
            self.last = (last_x, last_y);
        } else {
            if matches!(drag.grip, Grip::West | Grip::East) {
                dy = 0;
            }
            if matches!(drag.grip, Grip::North | Grip::South) {
                dx = 0;
            }
            // The moving side stays inside the stage, and the pointer keeps
            // its place relative to the area.
            self.last.0 += dx;
            if self.last.0 > max_x {
                dx -= self.last.0 - max_x;
                self.last.0 = max_x;
            } else if self.last.0 < min_x {
                dx -= self.last.0 - min_x;
                self.last.0 = min_x;
            }
            self.last.1 += dy;
            if self.last.1 > max_y {
                dy -= self.last.1 - max_y;
                self.last.1 = max_y;
            } else if self.last.1 < min_y {
                dy -= self.last.1 - min_y;
                self.last.1 = min_y;
            }
            // A handle dragged past the opposite side becomes that side's.
            drag.grip = if self.last.0 > self.start.0 {
                match drag.grip {
                    Grip::NorthWest => Grip::NorthEast,
                    Grip::SouthWest => Grip::SouthEast,
                    Grip::West => Grip::East,
                    other => other,
                }
            } else {
                match drag.grip {
                    Grip::NorthEast => Grip::NorthWest,
                    Grip::SouthEast => Grip::SouthWest,
                    Grip::East => Grip::West,
                    other => other,
                }
            };
            drag.grip = if self.last.1 > self.start.1 {
                match drag.grip {
                    Grip::NorthWest => Grip::SouthWest,
                    Grip::NorthEast => Grip::SouthEast,
                    Grip::North => Grip::South,
                    other => other,
                }
            } else {
                match drag.grip {
                    Grip::SouthWest => Grip::NorthWest,
                    Grip::SouthEast => Grip::NorthEast,
                    Grip::South => Grip::North,
                    other => other,
                }
            };
        }
        drag.from.x += f64::from(dx);
        drag.from.y += f64::from(dy);
        self.drag = Some(drag);
        drag.grip
    }

    /// A release (`_onRelease` 513–526). Only the drag's own button ends it.
    pub fn release(&mut self, button: Button, stage: Rect) -> DragEnd {
        match self.drag {
            Some(drag) if drag.button == button => {
                self.stop_drag(stage);
                DragEnd::Ended
            }
            _ => DragEnd::Ignored,
        }
    }

    /// Ends a drag (`stopDrag` 380–424). A new area that was clicked rather
    /// than drawn grows [`CLICK_GROWTH`] each way and is kept inside the
    /// stage, so a stray click leaves something visible to work with.
    pub fn stop_drag(&mut self, stage: Rect) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if drag.grip != Grip::Crosshair || self.start != self.last {
            return;
        }
        let offset = (CLICK_GROWTH * self.scale) as i32;
        self.start.0 -= offset;
        self.start.1 -= offset;
        self.last.0 += offset;
        self.last.1 += offset;
        let (min_x, min_y) = (stage.x, stage.y);
        let (max_x, max_y) = (stage.right() - 1, stage.bottom() - 1);
        if self.start.0 < min_x {
            self.last.0 += min_x - self.start.0;
            self.start.0 = min_x;
        } else if self.last.0 > max_x {
            self.start.0 -= self.last.0 - max_x;
            self.last.0 = max_x;
        }
        if self.start.1 < min_y {
            self.last.1 += min_y - self.start.1;
            self.start.1 = min_y;
        } else if self.last.1 > max_y {
            self.start.1 -= self.last.1 - max_y;
            self.last.1 = max_y;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AreaSelector, Button, DragEnd, Grip};
    use crate::geometry::{Point, Rect};

    const STAGE: Rect = Rect::new(0, 0, 1280, 800);

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn area(rect: Rect) -> AreaSelector {
        AreaSelector::remembered(rect, 1.0)
    }

    #[test]
    fn a_new_selector_starts_in_the_middle_quarter_of_the_primary_output() {
        let mut selector = AreaSelector::new(1.0);
        selector.reset(Some(STAGE), Some(STAGE));
        // 3/8 and 5/8 of 1280 × 800: 480..799 and 300..499.
        assert_eq!(selector.geometry(), Rect::new(480, 300, 320, 200));
    }

    #[test]
    fn a_remembered_area_that_fits_is_kept_and_one_that_does_not_is_reset() {
        let mut kept = area(Rect::new(10, 20, 300, 200));
        kept.reset(Some(STAGE), Some(STAGE));
        assert_eq!(kept.geometry(), Rect::new(10, 20, 300, 200));

        // The outputs changed: the area hangs off the stage.
        let mut lost = area(Rect::new(1200, 20, 300, 200));
        lost.reset(Some(STAGE), Some(STAGE));
        assert_eq!(lost.geometry(), Rect::new(480, 300, 320, 200));
    }

    #[test]
    fn pointer_shapes_follow_handles_edges_and_the_inside() {
        let selector = area(Rect::new(100, 100, 201, 101)); // right 300, bottom 200
        assert_eq!(selector.grip_at(p(100.0, 100.0)), Grip::NorthWest);
        assert_eq!(selector.grip_at(p(310.0, 95.0)), Grip::NorthEast);
        assert_eq!(selector.grip_at(p(95.0, 205.0)), Grip::SouthWest);
        assert_eq!(selector.grip_at(p(300.0, 200.0)), Grip::SouthEast);
        assert_eq!(selector.grip_at(p(95.0, 150.0)), Grip::West);
        assert_eq!(selector.grip_at(p(305.0, 150.0)), Grip::East);
        assert_eq!(selector.grip_at(p(200.0, 95.0)), Grip::North);
        assert_eq!(selector.grip_at(p(200.0, 205.0)), Grip::South);
        assert_eq!(selector.grip_at(p(200.0, 150.0)), Grip::Move);
        assert_eq!(selector.grip_at(p(500.0, 500.0)), Grip::Crosshair);
        // Past the 10 px band.
        assert_eq!(selector.grip_at(p(85.0, 150.0)), Grip::Crosshair);
    }

    #[test]
    fn the_edge_band_grows_with_the_scale() {
        let mut selector = area(Rect::new(100, 100, 201, 101));
        assert_eq!(selector.grip_at(p(85.0, 150.0)), Grip::Crosshair);
        selector.set_scale(2.0);
        assert_eq!(selector.grip_at(p(85.0, 150.0)), Grip::West);
    }

    #[test]
    fn dragging_outside_draws_a_new_area_in_either_direction() {
        let mut selector = area(Rect::new(100, 100, 50, 50));
        assert!(selector.press(p(600.5, 400.7), Button::Primary, false));
        assert_eq!(selector.motion(p(500.2, 300.9), STAGE), Grip::Crosshair);
        assert_eq!(selector.release(Button::Primary, STAGE), DragEnd::Ended);
        assert_eq!(selector.geometry(), Rect::new(500, 300, 101, 101));
    }

    #[test]
    fn the_secondary_button_or_ctrl_draws_a_new_area_even_inside() {
        let mut right = area(Rect::new(100, 100, 200, 200));
        right.press(p(150.0, 150.0), Button::Secondary, false);
        right.motion(p(160.0, 170.0), STAGE);
        right.release(Button::Secondary, STAGE);
        assert_eq!(right.geometry(), Rect::new(150, 150, 11, 21));

        let mut ctrl = area(Rect::new(100, 100, 200, 200));
        ctrl.press(p(150.0, 150.0), Button::Primary, true);
        ctrl.motion(p(140.0, 140.0), STAGE);
        ctrl.release(Button::Primary, STAGE);
        assert_eq!(ctrl.geometry(), Rect::new(140, 140, 11, 11));
    }

    #[test]
    fn a_click_without_a_drag_makes_a_square_kept_inside_the_stage() {
        let mut selector = area(Rect::new(100, 100, 50, 50));
        selector.press(p(640.0, 400.0), Button::Primary, false);
        selector.release(Button::Primary, STAGE);
        assert_eq!(selector.geometry(), Rect::new(620, 380, 41, 41));

        let mut corner = area(Rect::new(100, 100, 50, 50));
        corner.press(p(3.0, 1278.0f64.min(799.0)), Button::Primary, false);
        corner.release(Button::Primary, STAGE);
        assert_eq!(corner.geometry(), Rect::new(0, 759, 41, 41));

        let mut scaled = AreaSelector::remembered(Rect::new(100, 100, 50, 50), 2.0);
        scaled.press(p(640.0, 400.0), Button::Primary, false);
        scaled.release(Button::Primary, STAGE);
        assert_eq!(scaled.geometry(), Rect::new(600, 360, 81, 81));
    }

    #[test]
    fn moving_keeps_the_size_at_the_edge_and_the_pointer_slides() {
        let mut selector = area(Rect::new(100, 100, 200, 100));
        selector.press(p(200.0, 150.0), Button::Primary, false);
        assert_eq!(selector.motion(p(150.0, 150.0), STAGE), Grip::Move);
        assert_eq!(selector.geometry(), Rect::new(50, 100, 200, 100));
        // 100 px further left: the area stops at 0 and keeps its width.
        selector.motion(p(50.0, 150.0), STAGE);
        assert_eq!(selector.geometry(), Rect::new(0, 100, 200, 100));
        // The pointer slid 50 px over the area; coming back right moves the
        // area only once the pointer is back where it held it.
        selector.motion(p(80.0, 150.0), STAGE);
        assert_eq!(selector.geometry(), Rect::new(0, 100, 200, 100));
        selector.motion(p(110.0, 150.0), STAGE);
        assert_eq!(selector.geometry(), Rect::new(10, 100, 200, 100));
        selector.release(Button::Primary, STAGE);
        assert!(!selector.is_dragging());
    }

    #[test]
    fn resizing_one_edge_moves_only_that_edge() {
        let mut selector = area(Rect::new(100, 100, 201, 101)); // right 300
        selector.press(p(305.0, 150.0), Button::Primary, false);
        assert_eq!(selector.motion(p(355.0, 190.0), STAGE), Grip::East);
        assert_eq!(selector.geometry(), Rect::new(100, 100, 251, 101));
        selector.release(Button::Primary, STAGE);

        let mut north = area(Rect::new(100, 100, 201, 101));
        north.press(p(200.0, 95.0), Button::Primary, false);
        north.motion(p(250.0, 45.0), STAGE);
        assert_eq!(north.geometry(), Rect::new(100, 50, 201, 151));
    }

    #[test]
    fn a_corner_resizes_both_edges_and_turns_over_past_the_opposite_side() {
        let mut selector = area(Rect::new(100, 100, 201, 101)); // 100..300 × 100..200
        selector.press(p(300.0, 200.0), Button::Primary, false);
        assert_eq!(selector.drag_grip(), Some(Grip::SouthEast));
        // Past the left and top sides: the handle is now the north-west one.
        assert_eq!(selector.motion(p(50.0, 60.0), STAGE), Grip::NorthWest);
        assert_eq!(selector.geometry(), Rect::new(50, 60, 51, 41));
        selector.release(Button::Primary, STAGE);
    }

    #[test]
    fn resizing_stops_at_the_stage() {
        let mut selector = area(Rect::new(100, 100, 201, 101));
        selector.press(p(300.0, 200.0), Button::Primary, false);
        selector.motion(p(5000.0, 5000.0), STAGE);
        assert_eq!(selector.geometry(), Rect::new(100, 100, 1180, 700));
    }

    #[test]
    fn a_drawn_area_stops_at_the_stage() {
        let mut selector = area(Rect::new(0, 0, 10, 10));
        selector.press(p(1000.0, 700.0), Button::Primary, false);
        selector.motion(p(2000.0, -50.0), STAGE);
        assert_eq!(selector.geometry(), Rect::new(1000, 0, 280, 701));
    }

    #[test]
    fn only_the_drags_own_button_ends_it_and_a_second_press_is_ignored() {
        let mut selector = area(Rect::new(100, 100, 50, 50));
        selector.press(p(600.0, 400.0), Button::Primary, false);
        assert!(!selector.press(p(10.0, 10.0), Button::Secondary, false));
        assert_eq!(
            selector.release(Button::Secondary, STAGE),
            DragEnd::Ignored
        );
        assert!(selector.is_dragging());
        assert_eq!(selector.release(Button::Primary, STAGE), DragEnd::Ended);
    }

    #[test]
    fn an_area_may_span_outputs_on_a_stage_that_does_not_start_at_zero() {
        let stage = Rect::new(-1280, 0, 3200, 1080);
        let mut selector = area(Rect::new(0, 0, 10, 10));
        selector.press(p(-100.0, 100.0), Button::Primary, false);
        selector.motion(p(100.0, 200.0), stage);
        selector.release(Button::Primary, stage);
        assert_eq!(selector.geometry(), Rect::new(-100, 100, 201, 101));
        let mut edge = area(Rect::new(-1280, 0, 100, 100));
        edge.press(p(-1230.0, 50.0), Button::Primary, false);
        edge.motion(p(-2000.0, 50.0), stage);
        assert_eq!(edge.geometry().x, -1280);
    }
}

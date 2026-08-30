//! Back and forward history for one pane.
//!
//! Two stacks and a current location, which is the model every browser and
//! file manager uses and the only one where "go back then go somewhere new"
//! behaves the way people expect. Visiting a new location clears the forward
//! stack, because the branch the user did not take is not reachable any more.
//!
//! The stacks are bounded. An unbounded history is a slow memory leak in a
//! window a user leaves open for a week.

use crate::location::Location;

/// How many steps each direction keeps.
pub const DEFAULT_HISTORY_LIMIT: usize = 128;

/// Where a pane has been.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct History {
    back: Vec<Location>,
    forward: Vec<Location>,
    current: Location,
    limit: usize,
}

impl History {
    pub fn new(current: Location) -> Self {
        Self {
            back: Vec::new(),
            forward: Vec::new(),
            current,
            limit: DEFAULT_HISTORY_LIMIT,
        }
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.max(1);
        self.trim();
        self
    }

    pub fn current(&self) -> &Location {
        &self.current
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// What the back and forward menus list, nearest first.
    pub fn back_entries(&self) -> impl Iterator<Item = &Location> {
        self.back.iter().rev()
    }

    pub fn forward_entries(&self) -> impl Iterator<Item = &Location> {
        self.forward.iter().rev()
    }

    /// Goes somewhere new.
    ///
    /// Navigating to the location already shown is a no-op rather than a
    /// history entry, so clicking the same folder twice does not need two
    /// presses of Back to leave.
    pub fn visit(&mut self, location: Location) -> bool {
        if location == self.current {
            return false;
        }
        self.back
            .push(std::mem::replace(&mut self.current, location));
        self.forward.clear();
        self.trim();
        true
    }

    pub fn back(&mut self) -> Option<&Location> {
        let previous = self.back.pop()?;
        self.forward
            .push(std::mem::replace(&mut self.current, previous));
        Some(&self.current)
    }

    pub fn forward(&mut self) -> Option<&Location> {
        let next = self.forward.pop()?;
        self.back.push(std::mem::replace(&mut self.current, next));
        Some(&self.current)
    }

    /// Goes to the parent of the current location, when it has one.
    ///
    /// A virtual root has no parent, so this answers `None` rather than
    /// inventing a hierarchy that puts Applications inside Home.
    pub fn go_to_parent(&mut self) -> Option<&Location> {
        let parent = self.current.parent()?;
        self.visit(parent);
        Some(&self.current)
    }

    /// Replaces the current location without adding a history step, which is
    /// what a restored session or an in-place path edit does.
    pub fn replace_current(&mut self, location: Location) {
        self.current = location;
    }

    fn trim(&mut self) {
        // The oldest end is dropped, so Back keeps working for the recent past
        // rather than failing at the point the limit was reached.
        if self.back.len() > self.limit {
            let excess = self.back.len() - self.limit;
            self.back.drain(0..excess);
        }
        if self.forward.len() > self.limit {
            let excess = self.forward.len() - self.limit;
            self.forward.drain(0..excess);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(path: &str) -> Location {
        Location::local(path).unwrap()
    }

    #[test]
    fn back_and_forward_walk_the_visited_locations() {
        let mut history = History::new(at("/home"));
        history.visit(at("/home/user"));
        history.visit(at("/home/user/Documents"));
        assert!(history.can_go_back());
        assert!(!history.can_go_forward());

        assert_eq!(history.back(), Some(&at("/home/user")));
        assert_eq!(history.back(), Some(&at("/home")));
        assert_eq!(history.back(), None);
        assert_eq!(history.forward(), Some(&at("/home/user")));
        assert_eq!(history.current(), &at("/home/user"));
    }

    #[test]
    fn going_somewhere_new_after_going_back_drops_the_forward_branch() {
        let mut history = History::new(at("/a"));
        history.visit(at("/b"));
        history.visit(at("/c"));
        history.back();
        assert!(history.can_go_forward());
        history.visit(at("/d"));
        assert!(!history.can_go_forward());
        assert_eq!(history.current(), &at("/d"));
    }

    #[test]
    fn revisiting_the_current_location_does_not_add_a_step() {
        let mut history = History::new(at("/a"));
        assert!(!history.visit(at("/a")));
        assert!(!history.can_go_back());
    }

    #[test]
    fn history_mixes_virtual_and_filesystem_locations() {
        let mut history = History::new(Location::Applications);
        history.visit(at("/home/user"));
        history.visit(Location::Trash(crate::location::TrashLocation::Root));
        assert_eq!(history.back(), Some(&at("/home/user")));
        assert_eq!(history.back(), Some(&Location::Applications));
    }

    #[test]
    fn a_virtual_root_has_no_parent_to_navigate_to() {
        let mut history = History::new(Location::Applications);
        assert_eq!(history.go_to_parent(), None);
        let mut nested = History::new(at("/home/user/Documents"));
        assert_eq!(nested.go_to_parent(), Some(&at("/home/user")));
        assert!(nested.can_go_back());
    }

    #[test]
    fn the_stacks_are_bounded_and_drop_the_oldest_entries() {
        let mut history = History::new(at("/0")).with_limit(3);
        for index in 1..10 {
            history.visit(at(&format!("/{index}")));
        }
        assert_eq!(history.back_entries().count(), 3);
        assert_eq!(history.back_entries().next(), Some(&at("/8")));
    }
}

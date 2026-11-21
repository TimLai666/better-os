//! Better Screenshot's rules, with no GPUI and no Wayland in them
//! (engineering/specs/screenshot.md). The capture crate and the window crate
//! carry them out; every decision a person could see is here, with tests.

pub mod area;
pub mod compose;
pub mod files;
pub mod geometry;
pub mod interface;
pub mod layout;
pub mod portal;
pub mod recording;
pub mod session;
pub mod words;

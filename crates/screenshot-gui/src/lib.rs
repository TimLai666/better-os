//! Better Screenshot, the screenshot and screen recording interface of the
//! Better OS session (engineering/specs/screenshot.md). Its rules are in
//! `screenshot-core`, its pixels come through `screenshot-capture`; this
//! crate is the surfaces, the bus, and the process.

pub mod app;
pub mod assets;
pub mod cli;
pub mod open;
pub mod platform;
pub mod restart;
pub mod service;
pub mod view;

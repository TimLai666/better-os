//! The Better Awake tray: a StatusNotifierItem and a compact popup menu.
//!
//! The tray is a client and nothing else. It holds no inhibitor, keeps no
//! session state of its own, and runs no shell command. Restarting it changes
//! nothing about a session the service owns, which is the point of the split.

pub mod client;
pub mod controller;
pub mod dbusmenu;
pub mod item;
pub mod labels;
pub mod localtime;
pub mod menu;
pub mod sni;

pub use client::{ClientError, ServiceClient, start_request, status_from_event};
pub use controller::{APPLICATION_BINARY, Activation, TrayController};
pub use dbusmenu::DbusMenu;
pub use item::StatusNotifierItem;
pub use labels::{Labels, Locale};
pub use localtime::UtcOffset;
pub use menu::{Menu, MenuAction, MenuItem, QuickOptions, build as build_menu};
pub use sni::{ITEM_PATH, MENU_PATH, TrayAvailability, register_and_verify};

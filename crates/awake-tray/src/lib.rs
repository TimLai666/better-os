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
pub mod menu;
pub mod notify;
pub mod sni;

pub use better_core::local_time::TimeZone;
pub use client::{
    ClientError, LowBatteryStop, ServiceClient, ServiceEvent, service_event, start_request,
    status_from_event,
};
pub use controller::{
    APPLICATION_BINARY, Activation, FALLBACK_BINARY, KEEP_AWAKE_PAGE, TrayController,
    UNTIL_A_TIME_PAGE, change_request, failure_text, window_command,
};
pub use dbusmenu::DbusMenu;
pub use item::StatusNotifierItem;
pub use labels::{Labels, Locale, LocaleLabels};
pub use menu::{
    Menu, MenuAction, MenuItem, QuickOptions, build as build_menu,
    build_offered as build_offered_menu,
};
pub use notify::{DesktopNotifier, NotifyError, claim_notifications, handle_event};
pub use sni::{
    ITEM_PATH, MENU_PATH, TrayAvailability, keep_registered, register_and_verify, serve,
};

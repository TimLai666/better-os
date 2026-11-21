//! The Better OS session's launcher (ticket 203).
//!
//! GDM starts `/usr/libexec/better-os/session/start` from
//! `/usr/share/wayland-sessions/better-os.desktop`. The launcher:
//!
//! 1. sets the session's environment ([`environment::launch_environment`]),
//! 2. starts `cosmic-comp` as its child with a session socket
//!    ([`compositor`]); `cosmic-comp` tells it `WAYLAND_DISPLAY` and `DISPLAY`
//!    once it is up,
//! 3. imports the variables into the systemd user manager and D-Bus
//!    activation and starts [`TARGET`] ([`lifecycle::begin`]),
//! 4. starts the children that must be inside the session's own scope
//!    ([`children`]: the locker),
//! 5. waits; on logout ([`callers`]), a signal, or the compositor ending it
//!    stops the target, ends its children and the compositor, and puts back
//!    what it imported ([`lifecycle::stop_units`], [`lifecycle::restore`]).
//!
//! `cosmic-comp` is the launcher's child and not the launcher's successor by
//! `exec`: a `cosmic-comp` given a program to run is in kiosk mode, which
//! turns every key binding off (`cosmic-comp` `src/config/mod.rs`, `load`,
//! "Kiosk mode disables shortcuts"). `cosmic-session` starts it the same way.
//!
//! After an update of this package the launcher starts its new program in
//! its own place ([`resume`]); its children stay its children across the exec.

pub mod callers;
pub mod children;
pub mod compositor;
pub mod environment;
pub mod lifecycle;
pub mod resume;

/// The systemd user target the session's units hang from.
pub const TARGET: &str = "better-os-session.target";

/// systemd's target every desktop session binds to; every existing Better OS
/// user unit is `PartOf=` and `WantedBy=` it.
pub const GRAPHICAL_SESSION_TARGET: &str = "graphical-session.target";

/// The package that ships the launcher, which decides when an update of it
/// has finished.
pub const PACKAGE: &str = "better-os-session";

/// Where the package installs the launcher.
pub const INSTALLED_PATH: &str = "/usr/libexec/better-os/session/start";

/// The compositor, from the pinned closure (ADR 0038 C2).
pub const COMPOSITOR: &str = "/usr/bin/cosmic-comp";

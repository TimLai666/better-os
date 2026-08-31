//! The typed action-invocation boundary between a recognized gesture and the
//! desktop.
//!
//! `touchpad-gestures` decides that a gesture happened. This crate is the only
//! place that asks the session to do something about it, and it asks in
//! [`better_actions::DesktopAction`] values and a progress fraction — never in
//! a string that came from configuration. There is no method on
//! [`SessionAdapter`] that takes free text, so a configuration file has no
//! route to one.
//!
//! Two implementations exist, and both are narrow:
//!
//! - [`MockSessionAdapter`] records what it was asked to do and changes
//!   nothing. It is what every test runs against and what the shipped Test
//!   gestures mode uses, so the screen and the tests exercise the same code.
//! - [`launcher::LauncherActivationAdapter`] sends the two Better Launcher
//!   actions through `launcher-platform`'s existing activation path. It reuses
//!   that path rather than reinventing it, reports every other action
//!   unsupported, and is behind the `launcher-activation` feature so nothing
//!   here needs a session bus to build or to test.
//!
//! - [`gnome::GnomeShellAdapter`] performs the actions GNOME Shell owns —
//!   overview, show desktop, and switching workspaces — through the adapter
//!   extension in `adapters/gnome-shell-touchpad/`, and is the one route that
//!   can also ask the desktop to stop claiming the same fingers. It is written
//!   against the [`gnome::ShellBridge`] seam rather than against a bus, so all
//!   of its behaviour is tested with no shell anywhere near it; the session-bus
//!   transport under it is behind the `gnome-shell` feature.
//! - [`routing::RoutingAdapter`] puts the two together, sending each action to
//!   the first route that declares it.
//!
//! Which backend reaches the desktop is [ADR
//! 0012](../../../docs/decisions/0012-touchpad-gesture-backend.md); the GNOME
//! Shell adapter it chose is what this crate now implements.

pub mod adapter;
#[cfg(feature = "gnome-shell")]
pub mod bus;
pub mod gnome;
#[cfg(feature = "launcher-activation")]
pub mod launcher;
pub mod mock;
pub mod routing;

pub use adapter::{
    AdapterDescription, BindOutcome, GesturePhase, GestureProgress, InvocationOutcome,
    SessionAdapter, SuppressionOutcome, VerificationResult,
};
pub use gnome::{
    FakeShellBridge, GnomeShellAdapter, RecordedShellEvents, ShellBridge, ShellCapabilities,
    ShellError, ShellEvents, ShellGestureEvent, ShellRequest, WorkspaceDirection,
};
pub use mock::{Invocation, MockSessionAdapter};
pub use routing::RoutingAdapter;

#[cfg(feature = "gnome-shell")]
pub use bus::{SessionBusShell, SessionBusShellEvents};

#[cfg(feature = "launcher-activation")]
pub use launcher::{LauncherActivationAdapter, launcher_sample};

#[cfg(test)]
mod tests {
    /// The rule this crate exists to keep: nothing here runs a program, and
    /// nothing here takes a command from configuration. The trait's signatures
    /// already make that true; asserting it over the source catches the helper
    /// that would quietly add a second route.
    #[test]
    fn no_adapter_can_run_a_program() {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for entry in std::fs::read_dir(&source).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                continue;
            }
            let text: String = std::fs::read_to_string(&path)
                .unwrap()
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                // A `#[cfg(test)]` module is not shipped source, and this one
                // names the forbidden tokens on purpose.
                .take_while(|line| !line.contains("mod tests"))
                .collect::<Vec<_>>()
                .join("\n");
            for forbidden in ["Command::new", "process::Command", "gsettings", "sh -c"] {
                assert!(
                    !text.contains(forbidden),
                    "{} names {forbidden}",
                    path.display()
                );
            }
        }
    }
}

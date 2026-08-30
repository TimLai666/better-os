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
//! No adapter in this build reaches GNOME Shell. Which one eventually does is
//! [ADR 0012](../../../docs/decisions/0012-touchpad-gesture-backend.md); this
//! ticket records the decision and does not implement it.

pub mod adapter;
#[cfg(feature = "launcher-activation")]
pub mod launcher;
pub mod mock;

pub use adapter::{
    AdapterDescription, BindOutcome, GesturePhase, GestureProgress, InvocationOutcome,
    SessionAdapter, VerificationResult,
};
pub use mock::{Invocation, MockSessionAdapter};

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

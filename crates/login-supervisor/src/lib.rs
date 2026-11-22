//! The Better OS login supervisor (design/login-screen-spec.md section
//! 5.10, ticket 267): the display manager while the Better OS login screen
//! is chosen. It makes sure that within 80 seconds of starting, and of
//! every session's end, either the login screen accepts input or a text
//! login does, with Plymouth gone and a journal line for every stage that
//! did not pass.
//!
//! - [`rules`]: every stage, deadline, retry and the fall back, with no I/O;
//! - [`world`]: the loop around them, over the [`world::World`] edges;
//! - [`read`]: what it reads from the machine, each from a path a test can
//!   point elsewhere;
//! - [`system`]: the edges on a running system;
//! - [`state`]: the state file, `/run/better-os/login-path.json`;
//! - [`text`]: the text login's message and `better-os-login-status`'s
//!   words;
//! - [`bus`]: `org.betteros.LoginSupervisor1`, read-only.

pub mod bus;
pub mod read;
pub mod rules;
pub mod state;
pub mod system;
pub mod text;
pub mod world;

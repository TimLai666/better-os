//! How the overlay is asked to appear, and what happens when it is asked
//! twice.
//!
//! Every activation path in Issue #2 — the desktop entry, the global keyboard
//! shortcut, and later a gesture — ends at the same two types: an
//! [`ActivationRequest`] and the [`OverlayVisibility`] it arrives at. Resolving
//! those into an [`OverlayCommand`] is one function, so "pressing the shortcut
//! again closes it" cannot be implemented differently by each path.
//!
//! The second half of this module is the single-instance rule. A launcher that
//! opened a new window per activation would put two overlays on screen and
//! rebuild the index for each. [`SingleInstance`] makes the first process the
//! owner and turns every later launch into a request handed to it. The bus is
//! behind a trait, so the rule is tested with no bus at all and the real
//! session-bus implementation in [`crate::bus`] carries no policy of its own.

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::PlatformError;

/// A route by which the launcher can be opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivationPath {
    /// Five fingers inward. Only present when an adapter is attached, which is
    /// never in this build.
    Gesture,
    /// A configurable system shortcut, carried by the desktop's own keybinding
    /// settings rather than by an input grab.
    GlobalShortcut,
    /// The installed `.desktop` entry, from a dock, a panel, or the overview.
    DesktopEntry,
    /// Running the binary again while an instance is already running.
    SecondLaunch,
}

/// What an activation asks for.
///
/// [`ActivationRequest::Toggle`] is what a shortcut and a gesture send, because
/// the same input has to close what it opened. [`ActivationRequest::Open`] is
/// what a desktop entry sends: clicking a launcher icon should never be the
/// thing that closes the launcher.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ActivationRequest {
    #[default]
    Toggle,
    Open,
    Close,
}

/// What the overlay is doing when a request arrives.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OverlayVisibility {
    #[default]
    Hidden,
    Visible,
}

/// What the overlay should do about a request. `Ignore` exists so a redundant
/// request is a decision with a name rather than a silently dropped event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayCommand {
    Open,
    Close,
    Ignore,
}

impl ActivationRequest {
    /// The one place the toggle rule lives.
    pub fn resolve(self, visibility: OverlayVisibility) -> OverlayCommand {
        match (self, visibility) {
            (Self::Open, OverlayVisibility::Hidden) => OverlayCommand::Open,
            (Self::Open, OverlayVisibility::Visible) => OverlayCommand::Ignore,
            (Self::Close, OverlayVisibility::Visible) => OverlayCommand::Close,
            (Self::Close, OverlayVisibility::Hidden) => OverlayCommand::Ignore,
            (Self::Toggle, OverlayVisibility::Hidden) => OverlayCommand::Open,
            (Self::Toggle, OverlayVisibility::Visible) => OverlayCommand::Close,
        }
    }
}

/// A source of activation requests. The overlay treats a gesture adapter, the
/// session bus, and a test the same way through this trait.
pub trait ActivationSource {
    /// The next pending request, or `None` when there is nothing waiting. An
    /// implementation must not block.
    fn next_request(&self) -> Option<ActivationRequest>;
}

/// Whether this process got the well-known name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NameOwnership {
    Acquired,
    AlreadyOwned,
}

/// What this process is, once the name has been settled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstanceRole {
    /// This process owns the name. It opens the window and serves later
    /// activations.
    Primary,
    /// Another process owns the name and has been handed this launch's
    /// request. This process must exit without opening anything.
    Secondary,
}

/// The bus operations the single-instance rule needs, and nothing else.
pub trait NameRegistry {
    fn request_name(&self, name: &str) -> Result<NameOwnership, PlatformError>;
    /// Hands a request to the process that already owns the name.
    fn forward(&self, name: &str, request: ActivationRequest) -> Result<(), PlatformError>;
}

/// The single-instance rule: one overlay, however many times it is launched.
#[derive(Clone, Debug)]
pub struct SingleInstance {
    name: String,
}

impl Default for SingleInstance {
    fn default() -> Self {
        Self::new(Self::DEFAULT_NAME)
    }
}

impl SingleInstance {
    /// The well-known name Better Launcher owns. It follows the same
    /// `org.betteros.<Component><Version>` shape as the manager daemon and the
    /// awake service.
    pub const DEFAULT_NAME: &'static str = "org.betteros.Launcher1";

    /// The object path the activation interface is served at, derived from the
    /// name the same way every freedesktop application derives it.
    pub const OBJECT_PATH: &'static str = "/org/betteros/Launcher1";

    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Settles this process's role.
    ///
    /// A forward that fails is an error rather than a fallback to opening a
    /// window: a second overlay is a worse outcome than one reported failure,
    /// and silently opening one would make the single-instance promise
    /// conditional on the bus being healthy.
    pub fn acquire(
        &self,
        registry: &dyn NameRegistry,
        request: ActivationRequest,
    ) -> Result<InstanceRole, PlatformError> {
        match registry.request_name(&self.name)? {
            NameOwnership::Acquired => Ok(InstanceRole::Primary),
            NameOwnership::AlreadyOwned => {
                registry.forward(&self.name, request)?;
                Ok(InstanceRole::Secondary)
            }
        }
    }
}

/// A registry with no bus behind it: the first caller owns the name and every
/// later one is recorded as a forwarded request.
#[derive(Debug, Default)]
pub struct FakeNameRegistry {
    state: Mutex<FakeState>,
}

#[derive(Debug, Default)]
struct FakeState {
    owner: Option<String>,
    forwarded: Vec<ActivationRequest>,
    forward_fails: bool,
}

impl FakeNameRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// A registry whose owner is unreachable, which is what a primary instance
    /// dying between the name check and the call looks like.
    pub fn with_unreachable_owner(name: &str) -> Self {
        Self {
            state: Mutex::new(FakeState {
                owner: Some(name.to_string()),
                forwarded: Vec::new(),
                forward_fails: true,
            }),
        }
    }

    pub fn forwarded(&self) -> Vec<ActivationRequest> {
        self.state.lock().expect("registry lock").forwarded.clone()
    }
}

impl NameRegistry for FakeNameRegistry {
    fn request_name(&self, name: &str) -> Result<NameOwnership, PlatformError> {
        let mut state = self.state.lock().expect("registry lock");
        match &state.owner {
            Some(owner) if owner == name => Ok(NameOwnership::AlreadyOwned),
            _ => {
                state.owner = Some(name.to_string());
                Ok(NameOwnership::Acquired)
            }
        }
    }

    fn forward(&self, _name: &str, request: ActivationRequest) -> Result<(), PlatformError> {
        let mut state = self.state.lock().expect("registry lock");
        if state.forward_fails {
            return Err(PlatformError::ActivationFailed(
                "the running instance did not answer".to_string(),
            ));
        }
        state.forwarded.push(request);
        Ok(())
    }
}

/// A recording activation source, for driving the overlay in a test.
#[derive(Debug, Default)]
pub struct QueuedActivations {
    queue: Mutex<VecDeque<ActivationRequest>>,
}

impl QueuedActivations {
    pub fn new(requests: impl IntoIterator<Item = ActivationRequest>) -> Self {
        Self {
            queue: Mutex::new(requests.into_iter().collect()),
        }
    }
}

impl ActivationSource for QueuedActivations {
    fn next_request(&self) -> Option<ActivationRequest> {
        self.queue.lock().expect("queue lock").pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toggle_opens_a_hidden_overlay_and_closes_a_visible_one() {
        assert_eq!(
            ActivationRequest::Toggle.resolve(OverlayVisibility::Hidden),
            OverlayCommand::Open
        );
        assert_eq!(
            ActivationRequest::Toggle.resolve(OverlayVisibility::Visible),
            OverlayCommand::Close
        );
    }

    #[test]
    fn a_desktop_entry_never_closes_the_overlay_it_was_asked_to_open() {
        assert_eq!(
            ActivationRequest::Open.resolve(OverlayVisibility::Visible),
            OverlayCommand::Ignore
        );
        assert_eq!(
            ActivationRequest::Open.resolve(OverlayVisibility::Hidden),
            OverlayCommand::Open
        );
    }

    #[test]
    fn closing_something_already_closed_is_ignored_rather_than_reopened() {
        assert_eq!(
            ActivationRequest::Close.resolve(OverlayVisibility::Hidden),
            OverlayCommand::Ignore
        );
        assert_eq!(
            ActivationRequest::Close.resolve(OverlayVisibility::Visible),
            OverlayCommand::Close
        );
    }

    #[test]
    fn the_first_launch_owns_the_name_and_the_second_hands_over_its_request() {
        let registry = FakeNameRegistry::new();
        let instance = SingleInstance::default();

        assert_eq!(
            instance
                .acquire(&registry, ActivationRequest::Open)
                .unwrap(),
            InstanceRole::Primary
        );
        assert!(registry.forwarded().is_empty());

        assert_eq!(
            instance
                .acquire(&registry, ActivationRequest::Toggle)
                .unwrap(),
            InstanceRole::Secondary
        );
        assert_eq!(registry.forwarded(), vec![ActivationRequest::Toggle]);
    }

    #[test]
    fn a_third_launch_forwards_too_rather_than_stealing_the_name() {
        let registry = FakeNameRegistry::new();
        let instance = SingleInstance::default();
        for _ in 0..3 {
            instance
                .acquire(&registry, ActivationRequest::Toggle)
                .unwrap();
        }
        assert_eq!(registry.forwarded().len(), 2);
    }

    #[test]
    fn an_unreachable_owner_is_reported_instead_of_opening_a_second_overlay() {
        let registry = FakeNameRegistry::with_unreachable_owner(SingleInstance::DEFAULT_NAME);
        let error = SingleInstance::default()
            .acquire(&registry, ActivationRequest::Toggle)
            .unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("launcher.platform.error.activation_failed"),
            "{error}"
        );
    }

    #[test]
    fn the_well_known_name_and_object_path_agree_with_each_other() {
        assert_eq!(SingleInstance::DEFAULT_NAME, "org.betteros.Launcher1");
        assert_eq!(
            SingleInstance::OBJECT_PATH,
            format!("/{}", SingleInstance::DEFAULT_NAME.replace('.', "/"))
        );
    }

    #[test]
    fn a_queued_source_hands_requests_back_in_the_order_they_arrived() {
        let source = QueuedActivations::new([ActivationRequest::Open, ActivationRequest::Close]);
        assert_eq!(source.next_request(), Some(ActivationRequest::Open));
        assert_eq!(source.next_request(), Some(ActivationRequest::Close));
        assert_eq!(source.next_request(), None);
    }
}

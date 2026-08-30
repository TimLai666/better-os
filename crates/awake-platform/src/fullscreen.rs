//! Fullscreen and presentation state — which this crate cannot detect, and says
//! so.
//!
//! Issue #13 lists fullscreen as a trigger "where reliably detectable", and
//! ticket 26 leaves "whether fullscreen detection requires a minimal GNOME
//! adapter" as a deferred decision needing an ADR. So this provider ships
//! reporting itself unavailable, with the explanation naming what is missing.
//! That is the ticket's own rule applied to itself: a provider unavailable on
//! this platform shows an explanation rather than an inert control, and the
//! alternative — quietly omitting the condition from the rule editor — would
//! leave a user wondering why the feature the issue promised is not there.
//!
//! # Why there is nothing to read
//!
//! Under X11 a fullscreen window is discoverable from `_NET_WM_STATE`. Under
//! Wayland there is no such protocol: a client's fullscreen state is known to
//! the compositor and to nobody else, deliberately, and the only ways to learn
//! it are a compositor-specific interface such as the GNOME Shell D-Bus
//! introspection API, or the `org.freedesktop.portal.Inhibit` session-state
//! signal. Both are real options; both are a desktop-environment dependency this
//! ticket is explicitly not allowed to pick without an ADR. Reading `/proc` and
//! `/sys` cannot answer it at all, which is why there is no fixture-driven
//! implementation hiding behind a feature flag here.

use awake_core::{Observations, ProviderKind};

use crate::provider::{Cadence, TriggerProvider};

/// The stable key this provider reports. The GUI turns it into a sentence
/// naming the ADR; the key itself never carries prose.
pub const FULLSCREEN_UNAVAILABLE: &str = "awake.provider.fullscreen_needs_compositor_adapter";

/// Reports that fullscreen state cannot be detected here.
#[derive(Clone, Copy, Debug, Default)]
pub struct FullscreenProvider;

impl TriggerProvider for FullscreenProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Fullscreen
    }

    fn cadence(&self) -> Cadence {
        // Saying "I cannot answer" costs nothing, so this must never be a poll.
        Cadence::Free
    }

    fn sample(&mut self, _now_unix_seconds: u64, into: &mut Observations) {
        into.mark_unavailable(ProviderKind::Fullscreen, FULLSCREEN_UNAVAILABLE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use awake_core::{Condition, Truth, evaluate_condition};

    #[test]
    fn the_fullscreen_provider_reports_itself_unavailable_with_a_reason() {
        let mut observations = Observations::at(1_000);
        FullscreenProvider.sample(1_000, &mut observations);

        assert_eq!(observations.fullscreen_active, None);
        assert_eq!(
            observations
                .availability_of(ProviderKind::Fullscreen)
                .explanation(),
            Some(FULLSCREEN_UNAVAILABLE)
        );
    }

    #[test]
    fn a_fullscreen_condition_is_unknown_and_so_never_keeps_the_machine_awake() {
        let mut observations = Observations::at(1_000);
        FullscreenProvider.sample(1_000, &mut observations);

        assert_eq!(
            evaluate_condition(&Condition::Fullscreen { active: true }, &observations),
            Truth::Unknown,
            "an undetectable condition must not resolve to true"
        );
        assert_eq!(
            evaluate_condition(&Condition::Fullscreen { active: false }, &observations),
            Truth::Unknown,
            "and it must not resolve to false either, or the UI cannot explain itself"
        );
    }

    #[test]
    fn reporting_nothing_costs_nothing() {
        assert_eq!(FullscreenProvider.cadence(), Cadence::Free);
    }
}

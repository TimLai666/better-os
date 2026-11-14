//! What a session prevents, and what several sessions prevent together.

use serde::{Deserialize, Serialize};

use crate::session::{Reason, Session, SessionId, SessionOrigin};

/// The things a session can hold off, recorded per session so the merged
/// answer can always be explained by naming the session that asked for it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionPolicy {
    pub prevent_system_suspend: bool,
    pub prevent_idle: bool,
    pub prevent_display_sleep: bool,
    pub prevent_automatic_lock: bool,
    /// "Also when the lid is closed": the service holds logind's
    /// `handle-lid-switch` lock as well, so closing the lid does not suspend.
    ///
    /// A `sleep` lock alone does not do that: logind's `LidSwitchIgnoreInhibited`
    /// defaults to `yes`, so it acts on the lid whatever `sleep` locks are held.
    /// Defaulted on read, so a file or document written before the field existed
    /// reads as "off", which is what those sessions did.
    #[serde(default)]
    pub prevent_lid_suspend: bool,
}

impl SessionPolicy {
    /// The default quick session, fixed by Issue #13: the machine stays up, the
    /// display is still allowed to turn off, and the screen is still allowed to
    /// lock. Idle handling is held off because that is what stops logind
    /// suspending the machine for inactivity; it is not a display setting.
    pub const fn quick_default() -> Self {
        Self {
            prevent_system_suspend: true,
            prevent_idle: true,
            prevent_display_sleep: false,
            prevent_automatic_lock: false,
            prevent_lid_suspend: false,
        }
    }

    /// What a session a person starts by hand is given unless they chose
    /// otherwise (ticket 109): the quick default, and closing the lid does not
    /// suspend either. A rule keeps [`Self::quick_default`], so an automatic
    /// session holds the lid only when its rule says so.
    pub const fn manual_default() -> Self {
        Self {
            prevent_lid_suspend: true,
            ..Self::quick_default()
        }
    }

    /// Whether this policy weakens the machine's own protections enough that
    /// the user must be told once before it is applied. Keeping the display on
    /// costs power; leaving the screen unlocked costs security.
    pub const fn needs_security_confirmation(&self) -> bool {
        self.prevent_automatic_lock || self.prevent_display_sleep
    }

    /// Nothing held off at all.
    pub const fn none(&self) -> bool {
        !self.prevent_system_suspend
            && !self.prevent_idle
            && !self.prevent_display_sleep
            && !self.prevent_automatic_lock
            && !self.prevent_lid_suspend
    }

    /// Union: if any active session asks for something, it is held off.
    pub const fn union(self, other: Self) -> Self {
        Self {
            prevent_system_suspend: self.prevent_system_suspend || other.prevent_system_suspend,
            prevent_idle: self.prevent_idle || other.prevent_idle,
            prevent_display_sleep: self.prevent_display_sleep || other.prevent_display_sleep,
            prevent_automatic_lock: self.prevent_automatic_lock || other.prevent_automatic_lock,
            prevent_lid_suspend: self.prevent_lid_suspend || other.prevent_lid_suspend,
        }
    }
}

/// What a backend can actually do, so a request it cannot honor is reported as
/// unmet rather than quietly dropped.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendCapabilities {
    pub system_suspend: bool,
    pub idle: bool,
    pub display_sleep: bool,
    pub automatic_lock: bool,
    /// Whether the backend can stop the lid switch suspending the machine.
    /// Defaulted on read, so a backend report from before the field existed
    /// says it cannot.
    #[serde(default)]
    pub lid_switch: bool,
}

impl BackendCapabilities {
    pub const NONE: Self = Self {
        system_suspend: false,
        idle: false,
        display_sleep: false,
        automatic_lock: false,
        lid_switch: false,
    };

    /// Which parts of a policy this backend cannot deliver.
    pub fn gaps(&self, policy: &SessionPolicy) -> Vec<PolicyGap> {
        let mut gaps = Vec::new();
        if policy.prevent_system_suspend && !self.system_suspend {
            gaps.push(PolicyGap::SystemSuspend);
        }
        if policy.prevent_idle && !self.idle {
            gaps.push(PolicyGap::Idle);
        }
        if policy.prevent_display_sleep && !self.display_sleep {
            gaps.push(PolicyGap::DisplaySleep);
        }
        if policy.prevent_automatic_lock && !self.automatic_lock {
            gaps.push(PolicyGap::AutomaticLock);
        }
        if policy.prevent_lid_suspend && !self.lid_switch {
            gaps.push(PolicyGap::LidSwitch);
        }
        gaps
    }
}

/// One thing a session asked for that the active backend cannot do.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyGap {
    SystemSuspend,
    Idle,
    DisplaySleep,
    AutomaticLock,
    LidSwitch,
}

/// One line of the "why is this machine still awake" answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveReason {
    pub session: SessionId,
    pub origin: SessionOrigin,
    pub reason: Reason,
}

/// Every active session collapsed into the single answer the backend and the
/// menu both need.
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct EffectivePolicy {
    pub policy: SessionPolicy,
    /// The first threshold any active session would stop at, which is the
    /// highest of them. `None` means no active session watches the battery.
    pub battery_stop_percent: Option<u8>,
    pub reasons: Vec<ActiveReason>,
}

impl EffectivePolicy {
    pub fn merge<'a>(sessions: impl IntoIterator<Item = &'a Session>) -> Self {
        let mut merged = EffectivePolicy::default();
        for session in sessions {
            merged.policy = merged.policy.union(session.policy);
            merged.battery_stop_percent =
                merge_battery_threshold(merged.battery_stop_percent, session.battery_stop_percent);
            merged.reasons.push(ActiveReason {
                session: session.id,
                origin: session.origin,
                reason: session.reason.clone(),
            });
        }
        merged
    }

    pub fn is_idle(&self) -> bool {
        self.reasons.is_empty()
    }
}

/// Two thresholds combine into whichever stops first, so battery protection is
/// never weakened by starting a second session.
pub fn merge_battery_threshold(left: Option<u8>, right: Option<u8>) -> Option<u8> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{EndCondition, SessionId};

    fn session(id: u64, policy: SessionPolicy, battery: Option<u8>, reason: &str) -> Session {
        Session {
            id: SessionId(id),
            reason: Reason::new(reason).unwrap(),
            origin: SessionOrigin::Manual,
            policy,
            battery_stop_percent: battery,
            end: EndCondition::Indefinite,
            started_at_unix_seconds: 1_000,
            rule: None,
        }
    }

    #[test]
    fn no_sessions_prevent_nothing() {
        let merged = EffectivePolicy::merge(&[]);
        assert!(merged.is_idle());
        assert!(merged.policy.none());
        assert_eq!(merged.battery_stop_percent, None);
    }

    #[test]
    fn the_merged_policy_is_the_union_of_every_active_session() {
        let quiet = session(1, SessionPolicy::quick_default(), Some(20), "Build");
        let demanding = session(
            2,
            SessionPolicy {
                prevent_system_suspend: true,
                prevent_idle: true,
                prevent_display_sleep: true,
                prevent_automatic_lock: false,
                prevent_lid_suspend: false,
            },
            Some(20),
            "Presenting",
        );

        let merged = EffectivePolicy::merge(&[quiet, demanding]);
        assert!(merged.policy.prevent_system_suspend);
        assert!(merged.policy.prevent_display_sleep);
        assert!(
            !merged.policy.prevent_automatic_lock,
            "no session asked to stop locking, so locking stays on"
        );
        assert_eq!(merged.reasons.len(), 2);
        assert_eq!(merged.reasons[1].reason.as_str(), "Presenting");
    }

    #[test]
    fn merging_keeps_the_battery_threshold_that_stops_first() {
        let cautious = session(1, SessionPolicy::quick_default(), Some(30), "Build");
        let relaxed = session(2, SessionPolicy::quick_default(), Some(10), "Download");
        assert_eq!(
            EffectivePolicy::merge(&[cautious, relaxed]).battery_stop_percent,
            Some(30)
        );
    }

    #[test]
    fn a_session_that_opts_out_of_battery_protection_does_not_disable_another_ones() {
        let watched = session(1, SessionPolicy::quick_default(), Some(20), "Build");
        let unwatched = session(2, SessionPolicy::quick_default(), None, "Download");
        assert_eq!(
            EffectivePolicy::merge(&[watched, unwatched]).battery_stop_percent,
            Some(20)
        );
    }

    #[test]
    fn a_backend_reports_what_it_cannot_deliver_rather_than_accepting_it() {
        let logind_shaped = BackendCapabilities {
            system_suspend: true,
            idle: true,
            display_sleep: false,
            automatic_lock: false,
            lid_switch: true,
        };
        assert!(
            logind_shaped
                .gaps(&SessionPolicy::quick_default())
                .is_empty()
        );
        assert_eq!(
            logind_shaped.gaps(&SessionPolicy {
                prevent_display_sleep: true,
                prevent_automatic_lock: true,
                ..SessionPolicy::quick_default()
            }),
            vec![PolicyGap::DisplaySleep, PolicyGap::AutomaticLock]
        );
        assert_eq!(
            BackendCapabilities::NONE.gaps(&SessionPolicy::quick_default()),
            vec![PolicyGap::SystemSuspend, PolicyGap::Idle]
        );
    }

    #[test]
    fn the_lid_choice_merges_like_every_other_field_and_is_on_only_for_manual_sessions() {
        assert!(SessionPolicy::manual_default().prevent_lid_suspend);
        assert!(!SessionPolicy::quick_default().prevent_lid_suspend);
        let lid = session(1, SessionPolicy::manual_default(), Some(20), "Download");
        let rule = session(2, SessionPolicy::quick_default(), Some(20), "Build");
        assert!(
            EffectivePolicy::merge(&[lid, rule.clone()])
                .policy
                .prevent_lid_suspend
        );
        assert!(!EffectivePolicy::merge(&[rule]).policy.prevent_lid_suspend);
        assert!(
            !SessionPolicy::manual_default().needs_security_confirmation(),
            "keeping awake with the lid closed costs battery, not the screen lock"
        );
    }

    #[test]
    fn a_backend_without_a_lid_lock_reports_the_lid_choice_as_unmet() {
        let without_lid = BackendCapabilities {
            system_suspend: true,
            idle: true,
            display_sleep: false,
            automatic_lock: false,
            lid_switch: false,
        };
        assert_eq!(
            without_lid.gaps(&SessionPolicy::manual_default()),
            vec![PolicyGap::LidSwitch]
        );
    }

    #[test]
    fn a_policy_written_before_the_lid_choice_existed_reads_as_lid_off() {
        let old: SessionPolicy = serde_json::from_str(
            r#"{"prevent_system_suspend":true,"prevent_idle":true,"prevent_display_sleep":false,"prevent_automatic_lock":false}"#,
        )
        .unwrap();
        assert_eq!(old, SessionPolicy::quick_default());
        let old: BackendCapabilities = serde_json::from_str(
            r#"{"system_suspend":true,"idle":true,"display_sleep":false,"automatic_lock":false}"#,
        )
        .unwrap();
        assert!(!old.lid_switch);
    }

    #[test]
    fn only_the_display_and_lock_choices_need_a_security_warning() {
        assert!(!SessionPolicy::quick_default().needs_security_confirmation());
        assert!(
            SessionPolicy {
                prevent_automatic_lock: true,
                ..SessionPolicy::quick_default()
            }
            .needs_security_confirmation()
        );
        assert!(
            SessionPolicy {
                prevent_display_sleep: true,
                ..SessionPolicy::quick_default()
            }
            .needs_security_confirmation()
        );
    }
}

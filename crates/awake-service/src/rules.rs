//! The rule engine as the service runs it: rules, providers, and history in one
//! place.
//!
//! `awake-core` decides what a rule means and `awake-platform` finds out what is
//! true; this is what puts them together, keeps the answer, and writes it down.
//! It holds no inhibitor and makes no session — it produces the list of rules
//! that currently match, and [`crate::engine::AwakeEngine`] is the only thing
//! that turns that list into sessions.

use awake_core::{
    Evaluation, Observations, ProviderKind, Rule, RuleError, RuleId, RuleSet, Suppression,
    TriggerSession,
};
use awake_platform::{ProviderSet, Roots};
use awake_store::history::{History, HistoryStore};
use awake_store::rules::RulesStore;

/// Rules, providers, history, and the last thing the providers said.
pub struct RuleDriver {
    rules: RuleSet,
    rules_store: RulesStore,
    providers: ProviderSet,
    history: History,
    history_store: HistoryStore,
    /// The last complete sample, kept so a status query can answer from it
    /// without re-reading every kernel interface.
    last_observations: Observations,
    /// Rules that match but could not be given a session, each with the reason.
    /// Kept so a rule that silently never fires is visible instead of invisible.
    refused: Vec<(RuleId, String)>,
    /// Whether a rules file that could not be read was moved aside, or a newer
    /// one refused. A stable key, reported rather than swallowed.
    load_detail: Option<String>,
}

impl RuleDriver {
    /// Loads the rules and history, and prepares the providers.
    ///
    /// Neither an unreadable rules file nor an unreadable history stops the
    /// service: it can still keep the machine awake on request, and saying so is
    /// better than refusing to start. What it must not do is pretend the files
    /// were fine, so the reason is kept and reported.
    pub fn load(rules_store: RulesStore, history_store: HistoryStore, roots: Roots) -> Self {
        let mut load_detail = None;

        let rules = match rules_store.load() {
            Ok(loaded) => {
                if loaded.recovered_corrupt_state.is_some() {
                    load_detail = Some("awake.store.recovered_corrupt_rules".to_string());
                }
                loaded.rule_set
            }
            Err(error) => {
                load_detail = Some(error.to_string());
                RuleSet::new()
            }
        };

        let history = match history_store.load() {
            Ok(loaded) => {
                if loaded.recovered_corrupt_state.is_some() && load_detail.is_none() {
                    load_detail = Some("awake.store.recovered_corrupt_history".to_string());
                }
                loaded.history
            }
            Err(error) => {
                if load_detail.is_none() {
                    load_detail = Some(error.to_string());
                }
                History::new()
            }
        };

        let mut providers = ProviderSet::new(roots, || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_secs())
                .unwrap_or_default()
        });
        providers.watch_paths(&rules);

        Self {
            rules,
            rules_store,
            providers,
            history,
            history_store,
            last_observations: Observations::default(),
            refused: Vec::new(),
            load_detail,
        }
    }

    pub fn rules(&self) -> &RuleSet {
        &self.rules
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn history_mut(&mut self) -> &mut History {
        &mut self.history
    }

    pub fn observations(&self) -> &Observations {
        &self.last_observations
    }

    pub fn load_detail(&self) -> Option<&str> {
        self.load_detail.as_deref()
    }

    pub fn refused(&self) -> &[(RuleId, String)] {
        &self.refused
    }

    pub fn set_refused(&mut self, refused: Vec<(RuleId, String)>) {
        self.refused = refused;
    }

    /// Whether this machine runs on a battery, which is what makes the battery
    /// stop threshold a default rather than an option.
    pub fn has_battery(&self) -> bool {
        self.providers.has_battery()
    }

    /// What every provider can and cannot do, for Diagnostics and for the rule
    /// editor's unavailable-control explanations.
    pub fn provider_reports(&mut self, now: u64) -> Vec<awake_platform::ProviderReport> {
        self.providers.reports(now)
    }

    pub fn cadence(&self, kind: ProviderKind) -> awake_platform::Cadence {
        self.providers.cadence(kind)
    }

    /// Samples the providers the rules need and evaluates every enabled rule.
    ///
    /// A pause whose moment has passed is cleared here, so the stored state does
    /// not keep a stale timestamp for the rest of the service's life.
    pub fn evaluate(&mut self, now: u64) -> Evaluation {
        if self.rules.expire_pause(now) {
            let _ = self.rules_store.save(&self.rules);
        }
        // Power is always read, whatever the rules say. The battery stop
        // threshold protects every session, so making the reading conditional on
        // some rule happening to mention the battery would make the safety
        // guarantee depend on what the user wrote — which is exactly backwards.
        self.last_observations = self.providers.sample_with(
            &self.rules,
            &[ProviderKind::AcPower, ProviderKind::BatteryPercent],
            now,
        );
        self.rules.evaluate(&self.last_observations, now)
    }

    /// Evaluates against every provider regardless of cadence.
    ///
    /// Used when a rule was just edited and by the rule editor's test mode, both
    /// of which need an answer now rather than at the next interval.
    pub fn evaluate_now(&mut self, now: u64) -> Evaluation {
        self.last_observations = self.providers.sample_all(now);
        self.rules.evaluate(&self.last_observations, now)
    }

    /// The sessions the currently matching rules want held.
    pub fn desired_sessions(&self, evaluation: &Evaluation) -> Vec<TriggerSession> {
        evaluation
            .active
            .iter()
            .filter_map(|active| {
                self.rules.rule(active.rule).map(|rule| TriggerSession {
                    rule: rule.id,
                    reason: rule.reason(),
                    policy: rule.policy,
                    battery_stop_percent: rule.battery_stop_percent,
                })
            })
            .collect()
    }

    pub fn suppression(&self, now: u64) -> Option<Suppression> {
        self.rules.suppression(now)
    }

    /// Tests one rule without starting anything.
    pub fn test_rule(&mut self, id: RuleId, now: u64) -> Result<awake_core::RuleTest, RuleError> {
        // Sampled in full first: a person pressing Test wants the answer for
        // right now, not the answer from whenever each provider last polled.
        self.last_observations = self.providers.sample_all(now);
        self.rules.test_rule(id, &self.last_observations, now)
    }

    /// Applies one edit and persists the result.
    ///
    /// The watched paths are re-registered on every edit, so a rule that stopped
    /// naming a folder stops costing a watch descriptor immediately rather than
    /// at the next restart.
    pub fn edit(&mut self, edit: RuleEdit) -> Result<(), RuleError> {
        match edit {
            RuleEdit::Create(rule) => {
                self.rules.add(*rule)?;
            }
            RuleEdit::Update { id, rule } => self.rules.replace(id, *rule)?,
            RuleEdit::Delete(id) => {
                self.rules.remove(id)?;
            }
            RuleEdit::SetEnabled { id, enabled } => self.rules.set_enabled(id, enabled)?,
            RuleEdit::Duplicate(id) => {
                self.rules.duplicate(id)?;
            }
            RuleEdit::Reorder { id, to_index } => self.rules.reorder(id, to_index)?,
            RuleEdit::SetPriority { id, priority } => self.rules.set_priority(id, priority)?,
            RuleEdit::Pause { seconds, now } => match seconds {
                Some(seconds) => self.rules.pause_for(seconds, now)?,
                None => self.rules.pause_until_resumed(),
            },
            RuleEdit::Resume => self.rules.resume(),
            RuleEdit::OverrideAll { confirmed } => self.rules.override_all(confirmed)?,
        }
        self.providers.watch_paths(&self.rules);
        self.save();
        Ok(())
    }

    pub fn save(&self) {
        // A failed write must not take the running sessions down with it. The
        // service keeps holding what it holds; the next write may well succeed.
        let _ = self.rules_store.save(&self.rules);
        let _ = self.history_store.save(&self.history);
    }

    pub fn save_history(&self) {
        let _ = self.history_store.save(&self.history);
    }
}

/// One change to the rule set, so the dispatch in `engine.rs` stays a
/// translation from the wire rather than a second copy of the rule semantics.
#[derive(Clone, Debug)]
pub enum RuleEdit {
    Create(Box<Rule>),
    Update { id: RuleId, rule: Box<Rule> },
    Delete(RuleId),
    SetEnabled { id: RuleId, enabled: bool },
    Duplicate(RuleId),
    Reorder { id: RuleId, to_index: usize },
    SetPriority { id: RuleId, priority: u8 },
    Pause { seconds: Option<u64>, now: u64 },
    Resume,
    OverrideAll { confirmed: bool },
}

impl RuleEdit {
    /// Whether this edit is one the tray drives, whose answer a client wants as
    /// a status rather than as a rule list.
    pub fn answers_with_status(&self) -> bool {
        matches!(
            self,
            RuleEdit::Pause { .. } | RuleEdit::Resume | RuleEdit::OverrideAll { .. }
        )
    }
}

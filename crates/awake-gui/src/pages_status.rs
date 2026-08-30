//! Status, Quick Sessions, Session Defaults, and Battery & Safety.
//!
//! These four share a question: what will happen to this machine, and who
//! decided it. Each one shows the answer the service reported, never one this
//! window inferred.

use awake_core::{SessionOrigin, SessionPolicy};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, IconName,
    button::{Button, ButtonVariants},
    tag::Tag,
    *,
};

use crate::{
    app::AwakeApp,
    i18n::{copy, fill},
    model::{BatteryView, ReasonRow, Section, remaining_label, started_label, suppression_label},
    settings::PresetLength,
};

/// The lengths the Extend action offers. Fixed rather than free-text, because
/// an extension is a decision made in a hurry.
const EXTEND_MINUTES: [u64; 3] = [15, 30, 60];

impl AwakeApp {
    // ---- 1. Status -----------------------------------------------------

    pub(crate) fn status_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let mut root = v_flex().gap_5().child(self.section_heading(
            Section::Status.title(c),
            Section::Status.subtitle(c),
            cx,
        ));
        if let Some(banner) = self.connection_banner(cx) {
            return root.child(banner).into_any_element();
        }
        if let Some(banner) = self.action_banner(cx) {
            root = root.child(banner);
        }
        let Some(status) = self.status.as_ref() else {
            return root
                .child(self.state_message(c.unknown, c.service_unreachable_detail, cx))
                .into_any_element();
        };

        // Every active reason, manual and per rule. A machine held awake by two
        // things must say so twice.
        let reasons = self.surface(
            v_flex()
                .gap_2()
                .child(self.card_title(c.active_reasons))
                .child(if status.reasons.is_empty() {
                    self.state_message(c.no_active_reasons, c.no_active_reasons_detail, cx)
                } else {
                    v_flex()
                        .gap_3()
                        .children(
                            status
                                .reasons
                                .iter()
                                .map(|reason| self.reason_row(reason, cx)),
                        )
                        .into_any_element()
                }),
            cx,
        );

        let policy = self.surface(
            v_flex()
                .gap_1()
                .child(self.card_title(c.effective_policy))
                .children(
                    status
                        .policy
                        .iter()
                        .map(|row| self.key_value(row.field.label(c), row.value(c), cx)),
                )
                .child(self.key_value(c.field_battery_threshold, status.battery.summary(c), cx)),
            cx,
        );

        let health = self.surface(
            v_flex()
                .gap_1()
                .child(self.card_title(c.inhibitor_health))
                .child(self.key_value(c.backend_name, status.backend.name.clone(), cx))
                .child(self.key_value(
                    c.available,
                    if status.backend.available {
                        c.yes
                    } else {
                        c.no
                    },
                    cx,
                ))
                .when_some(status.backend.detail.clone(), |view, detail| {
                    view.child(self.key_value(c.backend_unavailable_detail, detail, cx))
                })
                .when_some(status.attention.clone(), |view, attention| {
                    view.child(self.key_value(c.attention_required, attention, cx))
                })
                .when_some(
                    status.interrupted_previous_session.clone(),
                    |view, interrupted| {
                        view.child(self.key_value(c.interrupted_previous_session, interrupted, cx))
                    },
                ),
            cx,
        );

        let battery = self.surface(
            v_flex()
                .gap_1()
                .child(self.card_title(c.battery_safety))
                .child(self.battery_rows(cx)),
            cx,
        );

        let conflicts = self.surface(
            v_flex()
                .gap_2()
                .child(self.card_title(c.conflicts_heading))
                .child(if status.conflicts.is_empty() {
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(c.no_conflicts)
                        .into_any_element()
                } else {
                    v_flex()
                        .gap_2()
                        .children(status.conflicts.iter().map(|conflict| {
                            v_flex()
                                .gap_0p5()
                                .min_w_0()
                                .child(
                                    div()
                                        .min_w_0()
                                        .text_sm()
                                        .font_semibold()
                                        .child(conflict.explanation(c)),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(conflict.resolution_label(c)),
                                )
                                .when_some(conflict.overridden_note(c), |view, note| {
                                    view.child(
                                        div()
                                            .min_w_0()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(note),
                                    )
                                })
                        }))
                        .into_any_element()
                }),
            cx,
        );

        root.child(reasons)
            .child(policy)
            .child(health)
            .child(battery)
            .child(conflicts)
            .when_some(status.suppression, |view, suppression| {
                view.child(self.warning(suppression_label(suppression, c), cx))
            })
            .into_any_element()
    }

    /// One active reason, with the actions that belong to it. End, extend, and
    /// modify sit on the session they act on rather than on a global bar, so
    /// there is no way to end the wrong one.
    fn reason_row(&self, reason: &ReasonRow, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let session_id = reason.session_id;
        let manual = reason.origin == SessionOrigin::Manual;
        let explanation = self
            .status
            .as_ref()
            .map(|status| status.ending_explanation(session_id, c))
            .unwrap_or_default();

        v_flex()
            .w_full()
            .min_w_0()
            .gap_2()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_3()
                    .items_center()
                    .flex_wrap()
                    .child(
                        div()
                            .min_w(px(200.0))
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .font_semibold()
                            .child(reason.display_name().to_string()),
                    )
                    .child(
                        Tag::secondary()
                            .small()
                            .rounded_full()
                            .child(reason.origin_label(c)),
                    ),
            )
            .when_some(reason.started_at_unix_seconds, |view, started| {
                view.child(self.key_value(
                    c.session_started,
                    started_label(started, self.offset),
                    cx,
                ))
            })
            .when_some(reason.remaining, |view, remaining| {
                view.child(self.key_value(c.remaining, remaining_label(remaining, c), cx))
            })
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .min_w_0()
                    .child(
                        Button::new(SharedString::from(format!("end-{session_id}")))
                            .danger()
                            .label(if manual {
                                c.end_session
                            } else {
                                c.end_this_reason
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                // The manual session has a request of its own,
                                // so the menu's End can never take a rule's
                                // session with it by naming the wrong id.
                                if manual {
                                    this.end_manual_session(cx);
                                } else {
                                    this.end_session(session_id, cx);
                                }
                            })),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(c.extend_session),
                    )
                    .children(EXTEND_MINUTES.map(|minutes| {
                        Button::new(SharedString::from(format!("extend-{session_id}-{minutes}")))
                            .label(fill(c.extend_by, "minutes", &minutes.to_string()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.extend_session(session_id, minutes, cx)
                            }))
                    }))
                    .when(manual, |row| {
                        row.child(
                            Button::new(SharedString::from(format!("modify-{session_id}")))
                                .label(c.modify_session)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.modify_session(session_id, cx)
                                })),
                        )
                    }),
            )
            // What ending this actually leaves behind, said before it is
            // pressed rather than discovered afterwards.
            .child(
                div()
                    .min_w_0()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(explanation),
            )
            .into_any_element()
    }

    // ---- 2. Quick Sessions ---------------------------------------------

    pub(crate) fn quick_sessions_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let count = self.preferences.presets.len();
        let default_index = self.preferences.default_preset;

        v_flex()
            .gap_5()
            .child(self.section_heading(
                Section::QuickSessions.title(c),
                Section::QuickSessions.subtitle(c),
                cx,
            ))
            .when_some(self.action_banner(cx), |view, banner| view.child(banner))
            .when(!self.preferences_saved, |view| {
                view.child(self.warning(c.settings_storage_failed, cx))
            })
            .child(
                self.surface(
                    v_flex()
                        .gap_3()
                        .child(self.card_title(c.presets_heading))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.presets_help),
                        )
                        .children(
                            (0..count).map(|index| {
                                self.preset_row(index, index == default_index, count, cx)
                            }),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.add_preset),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .flex_wrap()
                                .children(PRESET_CHOICES.map(|length| {
                                    Button::new(SharedString::from(format!(
                                        "add-preset-{}",
                                        preset_key(length)
                                    )))
                                    .icon(IconName::Plus)
                                    .label(length.label(c))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.add_preset(length, cx)
                                    }))
                                }))
                                .child(
                                    Button::new("restore-presets")
                                        .label(c.restore_defaults)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.restore_default_presets(cx)
                                        })),
                                ),
                        ),
                    cx,
                ),
            )
            .child(
                self.surface(
                    v_flex()
                        .gap_3()
                        .child(self.card_title(c.default_session_policy))
                        .child(self.policy_choices(cx)),
                    cx,
                ),
            )
            .into_any_element()
    }

    fn preset_row(
        &self,
        index: usize,
        is_default: bool,
        count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let length = self.preferences.presets[index];
        h_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .py_2()
            .items_center()
            .flex_wrap()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .min_w(px(160.0))
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .font_semibold()
                    .child(length.label(c)),
            )
            .when(is_default, |row| {
                row.child(
                    Tag::primary()
                        .small()
                        .rounded_full()
                        .child(c.default_preset),
                )
            })
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new(SharedString::from(format!("preset-start-{index}")))
                            .primary()
                            .icon(IconName::Play)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.start_preset(index, cx)),
                            ),
                    )
                    .when(!is_default, |row| {
                        row.child(
                            Button::new(SharedString::from(format!("preset-default-{index}")))
                                .label(c.set_as_default)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_default_preset(index, cx)
                                })),
                        )
                    })
                    .child(
                        Button::new(SharedString::from(format!("preset-up-{index}")))
                            .icon(IconName::ArrowUp)
                            .label(c.move_up)
                            .disabled(index == 0)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.move_preset(index, -1, cx)),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("preset-down-{index}")))
                            .icon(IconName::ArrowDown)
                            .label(c.move_down)
                            .disabled(index + 1 >= count)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.move_preset(index, 1, cx)),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("preset-remove-{index}")))
                            .icon(IconName::Delete)
                            // The menu must keep one length, or it offers a
                            // submenu that starts nothing.
                            .disabled(count <= 1)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.remove_preset(index, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// The four policy switches, shared by Quick Sessions and Session Defaults
    /// because they are the same four decisions.
    fn policy_choices(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let policy = self.preferences.defaults.policy;
        v_flex()
            .gap_2()
            .child(self.policy_choice(
                "policy-suspend",
                c.prevent_system_suspend_label,
                policy.prevent_system_suspend,
                |policy| policy.prevent_system_suspend = !policy.prevent_system_suspend,
                cx,
            ))
            .child(self.policy_choice(
                "policy-idle",
                c.prevent_idle_label,
                policy.prevent_idle,
                |policy| policy.prevent_idle = !policy.prevent_idle,
                cx,
            ))
            .child(self.policy_choice(
                "policy-display",
                c.prevent_display_sleep_label,
                policy.prevent_display_sleep,
                |policy| policy.prevent_display_sleep = !policy.prevent_display_sleep,
                cx,
            ))
            .child(self.policy_choice(
                "policy-lock",
                c.prevent_automatic_lock_label,
                policy.prevent_automatic_lock,
                |policy| policy.prevent_automatic_lock = !policy.prevent_automatic_lock,
                cx,
            ))
            .when(policy.needs_security_confirmation(), |view| {
                view.child(self.warning(c.reduced_security_warning, cx))
            })
            .into_any_element()
    }

    fn policy_choice(
        &self,
        id: &'static str,
        label: &'static str,
        prevented: bool,
        change: fn(&mut SessionPolicy),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        h_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .py_1p5()
            .items_center()
            .justify_between()
            .flex_wrap()
            .child(
                div()
                    .min_w(px(200.0))
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .child(label),
            )
            .child(
                Button::new(id)
                    .label(if prevented { c.prevented } else { c.allowed })
                    .selected(prevented)
                    .when(prevented, |button| button.primary())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut policy = this.preferences.defaults.policy;
                        change(&mut policy);
                        this.set_default_policy(policy, cx);
                    })),
            )
            .into_any_element()
    }

    // ---- 4. Session Defaults -------------------------------------------

    pub(crate) fn session_defaults_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        v_flex()
            .gap_5()
            .child(self.section_heading(
                Section::SessionDefaults.title(c),
                Section::SessionDefaults.subtitle(c),
                cx,
            ))
            .when(!self.preferences_saved, |view| {
                view.child(self.warning(c.settings_storage_failed, cx))
            })
            .child(
                self.surface(
                    v_flex()
                        .gap_3()
                        .child(self.card_title(c.default_policy_heading))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.session_defaults_help),
                        )
                        .child(self.policy_choices(cx)),
                    cx,
                ),
            )
            .child(
                self.surface(
                    v_flex()
                        .gap_3()
                        .child(self.card_title(c.default_battery_threshold))
                        .child(self.default_battery_control(cx)),
                    cx,
                ),
            )
            .into_any_element()
    }

    /// The default threshold control, which is only offered on a machine that
    /// has a battery. A desktop is told so instead of being given a number that
    /// could never fire.
    fn default_battery_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let offers = self
            .status
            .as_ref()
            .map(|status| status.battery.offers_threshold())
            // With no answer from the service yet, the control is not drawn.
            // Guessing that a battery exists is exactly the mistake this
            // section is written to avoid.
            .unwrap_or(false);
        if !offers {
            return v_flex()
                .gap_2()
                .child(self.explanation(c.no_battery, cx))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(c.no_battery_detail),
                )
                .into_any_element();
        }

        let percent = self.preferences.defaults.battery_stop_percent;
        let value = match percent {
            Some(percent) => fill(c.percent_value, "percent", &percent.to_string()),
            None => c.battery_stop_off.to_string(),
        };
        h_flex()
            .gap_3()
            .items_center()
            .flex_wrap()
            .child(self.stepper(
                "default-battery",
                value,
                |this, _, cx| {
                    let next = match this.preferences.defaults.battery_stop_percent {
                        Some(percent) if percent > awake_ipc::MIN_BATTERY_STOP_PERCENT => {
                            Some(percent - 1)
                        }
                        // Stepping below the smallest threshold turns the
                        // protection off rather than clamping silently.
                        Some(_) => None,
                        None => None,
                    };
                    this.set_default_battery(next, cx);
                },
                |this, _, cx| {
                    let next = match this.preferences.defaults.battery_stop_percent {
                        Some(percent) if percent < awake_ipc::MAX_BATTERY_STOP_PERCENT => {
                            Some(percent + 1)
                        }
                        Some(percent) => Some(percent),
                        None => Some(awake_ipc::MIN_BATTERY_STOP_PERCENT),
                    };
                    this.set_default_battery(next, cx);
                },
                cx,
            ))
            .child(
                Button::new("default-battery-off")
                    .label(c.battery_stop_off)
                    .selected(percent.is_none())
                    .on_click(cx.listener(|this, _, _, cx| this.set_default_battery(None, cx))),
            )
            .into_any_element()
    }

    // ---- 5. Battery & Safety --------------------------------------------

    pub(crate) fn battery_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let mut root = v_flex().gap_5().child(self.section_heading(
            Section::Battery.title(c),
            Section::Battery.subtitle(c),
            cx,
        ));
        if let Some(banner) = self.connection_banner(cx) {
            root = root.child(banner);
        }

        root.child(
            self.surface(
                v_flex()
                    .gap_3()
                    .child(self.card_title(c.battery_stop_threshold))
                    .child(self.battery_rows(cx))
                    .child(self.default_battery_control(cx)),
                cx,
            ),
        )
        .child(
            self.surface(
                v_flex()
                    .gap_2()
                    .child(self.card_title(c.on_battery_rule))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(c.on_battery_rule_detail),
                    ),
                cx,
            ),
        )
        .child(
            self.surface(
                v_flex()
                    .gap_2()
                    .child(self.card_title(c.quit_warning))
                    .child(self.warning(c.quit_warning_detail, cx)),
                cx,
            ),
        )
        .into_any_element()
    }

    /// The battery readings, or the statement that there is no battery. Shared
    /// by Status and Battery & Safety so the two cannot disagree.
    fn battery_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let Some(status) = self.status.as_ref() else {
            return div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(c.unknown)
                .into_any_element();
        };
        match status.battery {
            BatteryView::NotApplicable => v_flex()
                .gap_2()
                .child(self.key_value(c.battery_stop_threshold, c.not_applicable, cx))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(c.no_battery_detail),
                )
                .into_any_element(),
            BatteryView::Present {
                percent,
                on_ac_power,
                ..
            } => v_flex()
                .gap_1()
                .child(self.key_value(
                    c.current_charge,
                    match percent {
                        Some(percent) => fill(c.percent_value, "percent", &percent.to_string()),
                        None => c.unknown.to_string(),
                    },
                    cx,
                ))
                .child(self.key_value(
                    c.on_ac_power,
                    match on_ac_power {
                        Some(true) => c.on_ac_power,
                        Some(false) => c.on_battery_power,
                        None => c.unknown,
                    },
                    cx,
                ))
                .child(self.key_value(c.battery_stop_threshold, status.battery.summary(c), cx))
                .into_any_element(),
        }
    }
}

/// The lengths the Add preset row offers, which are the ones a menu can carry
/// without becoming a list to read.
const PRESET_CHOICES: [PresetLength; 5] = [
    PresetLength::Minutes { minutes: 15 },
    PresetLength::Minutes { minutes: 30 },
    PresetLength::Minutes { minutes: 60 },
    PresetLength::Minutes { minutes: 180 },
    PresetLength::Indefinite,
];

fn preset_key(length: PresetLength) -> String {
    match length {
        PresetLength::Indefinite => "indefinite".to_string(),
        PresetLength::Minutes { minutes } => minutes.to_string(),
    }
}

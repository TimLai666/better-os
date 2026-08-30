//! Automatic Rules: the list, the AND/OR condition group editor, and Test.
//!
//! The one rule this file exists to enforce: a condition whose provider cannot
//! be read here is rendered as an explanation, never as a control that looks
//! like it works. `ConditionView` decides that; this file only obeys it.

use awake_core::{Combine, Condition, ProcessMatchKind, ProviderKind, Schedule, Weekday};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, IconName,
    button::{Button, ButtonVariants},
    input::Input,
    tag::Tag,
    *,
};

use crate::{
    app::AwakeApp,
    i18n::{Copy, copy, fill},
    model::{
        ConditionView, GroupView, RuleTestView, RuleView, Section, condition_summary,
        provider_label, suppression_label,
    },
};

/// A two-button choice on a condition that is simply true or false: "on AC" and
/// "on battery", "playing" and "silent".
///
/// The three fields travel together because they describe one choice — the two
/// words for it and the edit that applies it — and splitting them across a long
/// argument list made it possible to pass the labels the wrong way round.
struct BooleanChoice {
    true_label: &'static str,
    false_label: &'static str,
    change: fn(&mut Condition, bool),
}

impl AwakeApp {
    pub(crate) fn rules_section(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        if self.draft.is_some() {
            return self.rule_editor(window, cx);
        }

        let mut root = v_flex().gap_5().child(self.section_heading(
            Section::Rules.title(c),
            Section::Rules.subtitle(c),
            cx,
        ));
        if let Some(banner) = self.connection_banner(cx) {
            return root.child(banner).into_any_element();
        }
        if let Some(banner) = self.action_banner(cx) {
            root = root.child(banner);
        }

        let summary = self.status.as_ref().map(|status| {
            let sentence = fill(
                c.rules_summary,
                "enabled",
                &status.rules_enabled.to_string(),
            );
            fill(&sentence, "total", &status.rules_total.to_string())
        });
        let refused = self.status.as_ref().and_then(|status| {
            (status.rules_refused > 0)
                .then(|| fill(c.refused_rules, "count", &status.rules_refused.to_string()))
        });

        root = root
            .when_some(self.rules_suppression, |view, suppression| {
                view.child(self.warning(suppression_label(suppression, c), cx))
            })
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_3()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .child(
                        v_flex()
                            .min_w(px(220.0))
                            .flex_1()
                            .min_w_0()
                            .when_some(summary, |view, summary| {
                                view.child(div().text_sm().child(summary))
                            })
                            .when_some(refused, |view, refused| {
                                view.child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().warning)
                                        .child(refused),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                Button::new("rules-new")
                                    .primary()
                                    .icon(IconName::Plus)
                                    .label(c.new_rule)
                                    .on_click(
                                        cx.listener(|this, _, window, cx| {
                                            this.new_rule(window, cx)
                                        }),
                                    ),
                            )
                            .child(
                                Button::new("rules-pause-short")
                                    .label(c.pause_rules_short)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.pause_rules(Some(awake_core::PAUSE_SHORT_SECONDS), cx)
                                    })),
                            )
                            .child(
                                Button::new("rules-pause-long")
                                    .label(c.pause_rules_long)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.pause_rules(Some(awake_core::PAUSE_LONG_SECONDS), cx)
                                    })),
                            )
                            .child(
                                Button::new("rules-resume")
                                    .label(c.resume_rules)
                                    .on_click(cx.listener(|this, _, _, cx| this.resume_rules(cx))),
                            )
                            .child(
                                Button::new("rules-override")
                                    .danger()
                                    .label(c.override_all_rules)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.override_all_rules(cx)),
                                    ),
                            ),
                    ),
            );

        if self.rules.is_empty() {
            return root
                .child(self.state_message(c.no_rules, c.no_rules_detail, cx))
                .into_any_element();
        }

        let count = self.rules.len();
        root.children(
            self.rules
                .iter()
                .enumerate()
                .map(|(index, rule)| self.rule_row(rule, index, count, cx)),
        )
        .when_some(self.test.as_ref(), |view, test| {
            view.child(self.test_result(test, cx))
        })
        .into_any_element()
    }

    fn rule_row(
        &self,
        rule: &RuleView,
        index: usize,
        count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let rule_id = rule.rule_id;
        let enabled = rule.enabled;
        self.surface(
            v_flex()
                .gap_3()
                .min_w_0()
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
                                .text_lg()
                                .font_semibold()
                                .child(rule.name.clone()),
                        )
                        .child(
                            if enabled {
                                Tag::success()
                            } else {
                                Tag::secondary()
                            }
                            .small()
                            .rounded_full()
                            .child(rule.state_label(c)),
                        )
                        .child(
                            if rule.matching_now {
                                Tag::primary()
                            } else {
                                Tag::secondary()
                            }
                            .small()
                            .rounded_full()
                            .child(rule.matching_label(c)),
                        ),
                )
                .child(self.key_value(c.rule_priority, rule.priority.to_string(), cx))
                .child(self.priority_stepper(rule_id, rule.priority, cx))
                .child(
                    v_flex()
                        .gap_2()
                        .min_w_0()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{}: {}",
                                    c.groups_combine,
                                    GroupView::combine_label(rule.combine, c)
                                )),
                        )
                        .children(rule.groups.iter().enumerate().map(|(group_index, group)| {
                            self.group_summary(group, group_index, c, cx)
                        })),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            Button::new(SharedString::from(format!("rule-edit-{rule_id}")))
                                .label(c.edit)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.edit_rule(rule_id, window, cx)
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("rule-toggle-{rule_id}")))
                                .label(if enabled { c.disable } else { c.enable })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_rule_enabled(rule_id, !enabled, cx)
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("rule-duplicate-{rule_id}")))
                                .label(c.duplicate)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.duplicate_rule(rule_id, cx)
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("rule-test-{rule_id}")))
                                .label(c.test_rule)
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.test_rule(rule_id, cx)),
                                ),
                        )
                        .child(
                            Button::new(SharedString::from(format!("rule-up-{rule_id}")))
                                .icon(IconName::ArrowUp)
                                .label(c.move_up)
                                .disabled(index == 0)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.move_rule(rule_id, -1, cx)
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("rule-down-{rule_id}")))
                                .icon(IconName::ArrowDown)
                                .label(c.move_down)
                                .disabled(index + 1 >= count)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.move_rule(rule_id, 1, cx)
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("rule-delete-{rule_id}")))
                                .danger()
                                .label(c.delete)
                                .on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.delete_rule(rule_id, cx)
                                    }),
                                ),
                        ),
                ),
            cx,
        )
    }

    fn priority_stepper(&self, rule_id: u64, priority: u8, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .gap_2()
            .items_center()
            .child(
                Button::new(SharedString::from(format!("priority-down-{rule_id}")))
                    .icon(IconName::Minus)
                    .disabled(priority == 0)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_rule_priority(rule_id, priority.saturating_sub(10), cx)
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("priority-up-{rule_id}")))
                    .icon(IconName::Plus)
                    .disabled(priority == u8::MAX)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_rule_priority(rule_id, priority.saturating_add(10), cx)
                    })),
            )
            .into_any_element()
    }

    /// One group, read-only, in the list. A condition whose provider is missing
    /// carries its explanation here too, so a broken rule is visible without
    /// opening the editor.
    fn group_summary(
        &self,
        group: &GroupView,
        index: usize,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        v_flex()
            .min_w_0()
            .gap_1()
            .pl_3()
            .border_l_2()
            .border_color(cx.theme().border)
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{} · {}",
                        fill(c.group_heading, "index", &(index + 1).to_string()),
                        GroupView::combine_label(group.combine, c)
                    )),
            )
            .children(
                group
                    .conditions
                    .iter()
                    .map(|condition| self.condition_line(condition, c, cx)),
            )
            .into_any_element()
    }

    /// One condition, in one place, deciding once whether it may look editable.
    fn condition_line(
        &self,
        condition: &ConditionView,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match condition.explanation() {
            None => div()
                .min_w_0()
                .text_sm()
                .child(condition.summary.clone())
                .into_any_element(),
            Some(explanation) => v_flex()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .min_w_0()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(condition.summary.clone()),
                )
                .child(self.explanation(
                    format!(
                        "{} — {}: {}",
                        c.condition_unavailable,
                        provider_label(condition.provider, c),
                        explanation
                    ),
                    cx,
                ))
                .into_any_element(),
        }
    }

    fn test_result(&self, test: &RuleTestView, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        self.surface(
            v_flex()
                .gap_2()
                .min_w_0()
                .child(self.card_title(c.test_result))
                .child(self.key_value(
                    c.rule_conditions,
                    RuleTestView::truth_label(test.truth, c),
                    cx,
                ))
                .children(test.group_truths.iter().enumerate().map(|(index, truth)| {
                    self.key_value(
                        fill(c.group_heading, "index", &(index + 1).to_string()),
                        RuleTestView::truth_label(*truth, c),
                        cx,
                    )
                }))
                .child(div().text_sm().font_semibold().child(test.outcome_label(c)))
                .when(test.rule_disabled, |view| {
                    view.child(self.explanation(c.tested_rule_is_disabled, cx))
                })
                .when_some(test.suppression, |view, suppression| {
                    view.child(self.warning(suppression_label(suppression, c), cx))
                })
                .children(test.unavailable.iter().map(|provider| {
                    self.explanation(
                        format!(
                            "{}: {}",
                            provider_label(provider.kind, c),
                            provider
                                .explanation
                                .clone()
                                .unwrap_or_else(|| c.unknown.to_string())
                        ),
                        cx,
                    )
                })),
            cx,
        )
    }

    // ---- The editor ------------------------------------------------------

    fn rule_editor(&self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let Some(draft) = self.draft.as_ref() else {
            return div().into_any_element();
        };
        let creating = draft.rule_id.is_none();
        let group_count = draft.groups.len();

        v_flex()
            .gap_5()
            .child(self.section_heading(
                if creating {
                    c.rule_editor_new
                } else {
                    c.rule_editor_edit
                },
                Section::Rules.subtitle(c),
                cx,
            ))
            .when_some(draft.error.clone(), |view, error| {
                view.child(self.danger(error, cx))
            })
            .child(
                self.surface(
                    v_flex()
                        .gap_3()
                        .child(self.card_title(c.rule_name))
                        .child(Input::new(&draft.name))
                        .child(
                            h_flex()
                                .gap_3()
                                .items_center()
                                .flex_wrap()
                                .child(
                                    Button::new("draft-enabled")
                                        .label(if draft.enabled { c.enabled } else { c.disabled })
                                        .selected(draft.enabled)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.update_draft(
                                                |draft| draft.enabled = !draft.enabled,
                                                cx,
                                            )
                                        })),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(c.rule_priority),
                                )
                                .child(self.stepper(
                                    "draft-priority",
                                    draft.priority.to_string(),
                                    |this, _, cx| {
                                        this.update_draft(
                                            |draft| {
                                                draft.priority = draft.priority.saturating_sub(5)
                                            },
                                            cx,
                                        )
                                    },
                                    |this, _, cx| {
                                        this.update_draft(
                                            |draft| {
                                                draft.priority = draft.priority.saturating_add(5)
                                            },
                                            cx,
                                        )
                                    },
                                    cx,
                                )),
                        ),
                    cx,
                ),
            )
            .child(
                self.surface(
                    v_flex()
                        .gap_3()
                        .child(self.card_title(c.rule_conditions))
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .flex_wrap()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(c.groups_combine),
                                )
                                .child(self.choice(
                                    "draft-combine-all",
                                    c.match_all,
                                    draft.combine == Combine::All,
                                    |this, _, cx| {
                                        this.update_draft(|draft| draft.combine = Combine::All, cx)
                                    },
                                    cx,
                                ))
                                .child(self.choice(
                                    "draft-combine-any",
                                    c.match_any,
                                    draft.combine == Combine::Any,
                                    |this, _, cx| {
                                        this.update_draft(|draft| draft.combine = Combine::Any, cx)
                                    },
                                    cx,
                                )),
                        )
                        .children(
                            (0..group_count)
                                .map(|index| self.draft_group(index, group_count, c, cx)),
                        )
                        .child(
                            Button::new("draft-add-group")
                                .icon(IconName::Plus)
                                .label(c.add_group)
                                .disabled(group_count >= awake_core::MAX_GROUPS_PER_RULE)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.add_group(window, cx)),
                                ),
                        ),
                    cx,
                ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("draft-save")
                            .primary()
                            .label(c.save)
                            .on_click(cx.listener(|this, _, _, cx| this.save_draft(cx))),
                    )
                    .child(
                        Button::new("draft-cancel")
                            .label(c.cancel)
                            .on_click(cx.listener(|this, _, _, cx| this.close_draft(cx))),
                    ),
            )
            .into_any_element()
    }

    fn draft_group(
        &self,
        index: usize,
        group_count: usize,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(draft) = self.draft.as_ref() else {
            return div().into_any_element();
        };
        let Some(group) = draft.groups.get(index) else {
            return div().into_any_element();
        };
        let combine = group.combine;
        let condition_count = group.conditions.len();

        v_flex()
            .min_w_0()
            .gap_2()
            .p_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .items_center()
                    .flex_wrap()
                    .child(
                        div()
                            .min_w(px(120.0))
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .font_semibold()
                            .child(fill(c.group_heading, "index", &(index + 1).to_string())),
                    )
                    .child(self.choice(
                        "group-all",
                        c.match_all,
                        combine == Combine::All,
                        move |this, _, cx| {
                            this.update_draft(
                                |draft| {
                                    if let Some(group) = draft.groups.get_mut(index) {
                                        group.combine = Combine::All;
                                    }
                                },
                                cx,
                            )
                        },
                        cx,
                    ))
                    .child(self.choice(
                        "group-any",
                        c.match_any,
                        combine == Combine::Any,
                        move |this, _, cx| {
                            this.update_draft(
                                |draft| {
                                    if let Some(group) = draft.groups.get_mut(index) {
                                        group.combine = Combine::Any;
                                    }
                                },
                                cx,
                            )
                        },
                        cx,
                    ))
                    .child(
                        Button::new(SharedString::from(format!("group-remove-{index}")))
                            .icon(IconName::Delete)
                            .disabled(group_count <= 1)
                            .on_click(cx.listener(move |this, _, cx_window, cx| {
                                let _ = cx_window;
                                this.remove_group(index, cx)
                            })),
                    ),
            )
            .children(
                (0..condition_count)
                    .map(|condition| self.draft_condition_row(index, condition, c, cx)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(c.add_condition),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .children(ProviderKind::ALL.map(|provider| {
                        let available = self.provider_is_available(provider);
                        Button::new(SharedString::from(format!(
                            "add-condition-{index}-{}",
                            provider.as_key()
                        )))
                        .icon(IconName::Plus)
                        .label(provider_label(provider, c))
                        // A provider that cannot be read here is not offered as
                        // something to add. The list still shows it, so nobody
                        // is left wondering where the option went.
                        .disabled(!available)
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.add_condition(
                                    index,
                                    Self::default_condition(provider),
                                    window,
                                    cx,
                                )
                            },
                        ))
                    })),
            )
            .into_any_element()
    }

    fn provider_is_available(&self, provider: ProviderKind) -> bool {
        self.providers
            .iter()
            .find(|row| row.kind == provider)
            .map(|row| row.available)
            // A provider the service never mentioned is treated as available;
            // refusing to offer it because a report was silent would be a guess.
            .unwrap_or(true)
    }

    /// One condition inside the editor.
    ///
    /// This is the acceptance criterion in code: the view model is asked first,
    /// and a condition it marks unavailable never reaches the branch that draws
    /// operand controls.
    fn draft_condition_row(
        &self,
        group: usize,
        index: usize,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(condition) = self
            .draft
            .as_ref()
            .and_then(|draft| draft.groups.get(group))
            .and_then(|group| group.conditions.get(index))
        else {
            return div().into_any_element();
        };
        let presented = ConditionView::present(&condition.condition, &self.providers, c);

        if let Some(explanation) = presented.explanation() {
            return v_flex()
                .min_w_0()
                .gap_1()
                .py_2()
                .child(
                    div()
                        .min_w_0()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(presented.summary.clone()),
                )
                .child(self.explanation(
                    format!(
                        "{} — {}. {}: {}",
                        c.condition_unavailable,
                        provider_label(presented.provider, c),
                        c.condition_unavailable_detail,
                        explanation
                    ),
                    cx,
                ))
                .child(
                    // Removing it is the only action still offered, so a rule
                    // carried over from another machine can be repaired.
                    Button::new(SharedString::from(format!(
                        "condition-remove-{group}-{index}"
                    )))
                    .label(c.remove_condition)
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.remove_condition(group, index, cx)),
                    ),
                )
                .into_any_element();
        }

        v_flex()
            .min_w_0()
            .gap_2()
            .py_2()
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
                            .child(condition_summary(&condition.condition, c)),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "condition-remove-{group}-{index}"
                        )))
                        .label(c.remove_condition)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remove_condition(group, index, cx)
                        })),
                    ),
            )
            .child(self.condition_operands(group, index, c, cx))
            .into_any_element()
    }

    /// The operand controls for one editable condition.
    fn condition_operands(
        &self,
        group: usize,
        index: usize,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(draft_condition) = self
            .draft
            .as_ref()
            .and_then(|draft| draft.groups.get(group))
            .and_then(|group| group.conditions.get(index))
        else {
            return div().into_any_element();
        };
        let id = |suffix: &str| SharedString::from(format!("cond-{group}-{index}-{suffix}"));

        match &draft_condition.condition {
            Condition::ProcessRunning { matcher } => {
                let kind = matcher.kind;
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .when_some(draft_condition.text.as_ref(), |row, input| {
                        row.child(div().w(px(260.0)).max_w_full().child(Input::new(input)))
                    })
                    .child(
                        Button::new(id("executable"))
                            .label(c.matcher_executable)
                            .selected(kind == ProcessMatchKind::ExecutableName)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_condition(
                                    group,
                                    index,
                                    |condition| {
                                        if let Condition::ProcessRunning { matcher } = condition {
                                            *matcher = awake_core::ProcessMatcher::new(
                                                ProcessMatchKind::ExecutableName,
                                                matcher.as_str(),
                                            )
                                            .unwrap_or_else(|_| matcher.clone());
                                        }
                                    },
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new(id("desktop"))
                            .label(c.matcher_desktop_id)
                            .selected(kind == ProcessMatchKind::DesktopId)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_condition(
                                    group,
                                    index,
                                    |condition| {
                                        if let Condition::ProcessRunning { matcher } = condition {
                                            *matcher = awake_core::ProcessMatcher::new(
                                                ProcessMatchKind::DesktopId,
                                                matcher.as_str(),
                                            )
                                            .unwrap_or_else(|_| matcher.clone());
                                        }
                                    },
                                    cx,
                                )
                            })),
                    )
                    .into_any_element()
            }
            Condition::AcPower { connected } => self.boolean_operand(
                group,
                index,
                *connected,
                BooleanChoice {
                    true_label: c.condition_ac_connected,
                    false_label: c.condition_ac_disconnected,
                    change: |condition, value| {
                        if let Condition::AcPower { connected } = condition {
                            *connected = value;
                        }
                    },
                },
                cx,
            ),
            Condition::ExternalDisplay { connected } => self.boolean_operand(
                group,
                index,
                *connected,
                BooleanChoice {
                    true_label: c.condition_external_display_connected,
                    false_label: c.condition_external_display_disconnected,
                    change: |condition, value| {
                        if let Condition::ExternalDisplay { connected } = condition {
                            *connected = value;
                        }
                    },
                },
                cx,
            ),
            Condition::AudioPlayback { playing } => self.boolean_operand(
                group,
                index,
                *playing,
                BooleanChoice {
                    true_label: c.condition_audio_playing,
                    false_label: c.condition_audio_silent,
                    change: |condition, value| {
                        if let Condition::AudioPlayback { playing } = condition {
                            *playing = value;
                        }
                    },
                },
                cx,
            ),
            Condition::Fullscreen { active } => self.boolean_operand(
                group,
                index,
                *active,
                BooleanChoice {
                    true_label: c.condition_fullscreen_active,
                    false_label: c.condition_fullscreen_inactive,
                    change: |condition, value| {
                        if let Condition::Fullscreen { active } = condition {
                            *active = value;
                        }
                    },
                },
                cx,
            ),
            Condition::BatteryPercent { at_least, at_most } => h_flex()
                .gap_3()
                .flex_wrap()
                .items_center()
                .child(self.stepper(
                    "battery-low",
                    fill(c.percent_value, "percent", &at_least.to_string()),
                    move |this, _, cx| {
                        this.update_condition(
                            group,
                            index,
                            |condition| {
                                if let Condition::BatteryPercent { at_least, .. } = condition {
                                    *at_least = at_least.saturating_sub(5);
                                }
                            },
                            cx,
                        )
                    },
                    move |this, _, cx| {
                        this.update_condition(
                            group,
                            index,
                            |condition| {
                                if let Condition::BatteryPercent { at_least, at_most } = condition {
                                    *at_least = (*at_least + 5).min(*at_most);
                                }
                            },
                            cx,
                        )
                    },
                    cx,
                ))
                .child(self.stepper(
                    "battery-high",
                    fill(c.percent_value, "percent", &at_most.to_string()),
                    move |this, _, cx| {
                        this.update_condition(
                            group,
                            index,
                            |condition| {
                                if let Condition::BatteryPercent { at_least, at_most } = condition {
                                    *at_most = at_most.saturating_sub(5).max(*at_least);
                                }
                            },
                            cx,
                        )
                    },
                    move |this, _, cx| {
                        this.update_condition(
                            group,
                            index,
                            |condition| {
                                if let Condition::BatteryPercent { at_most, .. } = condition {
                                    *at_most = (*at_most + 5).min(100);
                                }
                            },
                            cx,
                        )
                    },
                    cx,
                ))
                .into_any_element(),
            Condition::CpuUtilizationAtLeast { percent } => self.stepper(
                "cpu-percent",
                fill(c.percent_value, "percent", &percent.to_string()),
                move |this, _, cx| {
                    this.update_condition(
                        group,
                        index,
                        |condition| {
                            if let Condition::CpuUtilizationAtLeast { percent } = condition {
                                *percent = percent.saturating_sub(5);
                            }
                        },
                        cx,
                    )
                },
                move |this, _, cx| {
                    this.update_condition(
                        group,
                        index,
                        |condition| {
                            if let Condition::CpuUtilizationAtLeast { percent } = condition {
                                *percent = (*percent + 5).min(100);
                            }
                        },
                        cx,
                    )
                },
                cx,
            ),
            Condition::NetworkThroughputAtLeast {
                kibibytes_per_second,
            } => self.stepper(
                "network-rate",
                kibibytes_per_second.to_string(),
                move |this, _, cx| {
                    this.update_condition(
                        group,
                        index,
                        |condition| {
                            if let Condition::NetworkThroughputAtLeast {
                                kibibytes_per_second,
                            } = condition
                            {
                                *kibibytes_per_second = kibibytes_per_second.saturating_sub(50);
                            }
                        },
                        cx,
                    )
                },
                move |this, _, cx| {
                    this.update_condition(
                        group,
                        index,
                        |condition| {
                            if let Condition::NetworkThroughputAtLeast {
                                kibibytes_per_second,
                            } = condition
                            {
                                *kibibytes_per_second += 50;
                            }
                        },
                        cx,
                    )
                },
                cx,
            ),
            Condition::NetworkInterfaceUp { .. } => draft_condition
                .text
                .as_ref()
                .map(|input| {
                    div()
                        .w(px(260.0))
                        .max_w_full()
                        .child(Input::new(input))
                        .into_any_element()
                })
                .unwrap_or_else(|| div().into_any_element()),
            Condition::WatchedPathActive { within_seconds, .. } => h_flex()
                .gap_3()
                .flex_wrap()
                .items_center()
                .when_some(draft_condition.text.as_ref(), |row, input| {
                    row.child(div().w(px(260.0)).max_w_full().child(Input::new(input)))
                })
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(c.watch_window),
                )
                .child(self.stepper(
                    "watch-window",
                    format!("{within_seconds} {}", c.second_unit),
                    move |this, _, cx| {
                        this.update_condition(
                            group,
                            index,
                            |condition| {
                                if let Condition::WatchedPathActive { within_seconds, .. } =
                                    condition
                                {
                                    *within_seconds = within_seconds.saturating_sub(60).max(1);
                                }
                            },
                            cx,
                        )
                    },
                    move |this, _, cx| {
                        this.update_condition(
                            group,
                            index,
                            |condition| {
                                if let Condition::WatchedPathActive { within_seconds, .. } =
                                    condition
                                {
                                    *within_seconds = (*within_seconds + 60)
                                        .min(awake_core::MAX_WATCH_WINDOW_SECONDS);
                                }
                            },
                            cx,
                        )
                    },
                    cx,
                ))
                .into_any_element(),
            Condition::TimeSchedule { schedule } => {
                self.schedule_operand(group, index, schedule.clone(), c, cx)
            }
        }
    }

    fn boolean_operand(
        &self,
        group: usize,
        index: usize,
        value: bool,
        choice: BooleanChoice,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let BooleanChoice {
            true_label,
            false_label,
            change,
        } = choice;
        h_flex()
            .gap_2()
            .flex_wrap()
            .child(
                Button::new(SharedString::from(format!("bool-true-{group}-{index}")))
                    .label(true_label)
                    .selected(value)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.update_condition(group, index, |condition| change(condition, true), cx)
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("bool-false-{group}-{index}")))
                    .label(false_label)
                    .selected(!value)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.update_condition(
                            group,
                            index,
                            |condition| change(condition, false),
                            cx,
                        )
                    })),
            )
            .into_any_element()
    }

    fn schedule_operand(
        &self,
        group: usize,
        index: usize,
        schedule: Schedule,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let from = schedule.from_minute_of_day;
        let to = schedule.to_minute_of_day;
        v_flex()
            .gap_2()
            .min_w_0()
            .child(
                h_flex()
                    .gap_1()
                    .flex_wrap()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(c.schedule_days),
                    )
                    .children(Weekday::ALL.map(|day| {
                        let selected = schedule.days.contains(&day);
                        Button::new(SharedString::from(format!(
                            "day-{group}-{index}-{}",
                            day.index()
                        )))
                        .label(weekday_label(day, c))
                        .selected(selected)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.update_condition(
                                group,
                                index,
                                move |condition| {
                                    if let Condition::TimeSchedule { schedule } = condition {
                                        let mut days = schedule.days.clone();
                                        if selected {
                                            days.retain(|existing| *existing != day);
                                        } else {
                                            days.push(day);
                                        }
                                        // A schedule with no days is refused by the
                                        // core, so the last day cannot be removed.
                                        if let Ok(next) = Schedule::new(
                                            days,
                                            schedule.from_minute_of_day,
                                            schedule.to_minute_of_day,
                                        ) {
                                            *schedule = next;
                                        }
                                    }
                                },
                                cx,
                            )
                        }))
                    })),
            )
            .child(
                h_flex()
                    .gap_3()
                    .flex_wrap()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(c.schedule_from),
                    )
                    .child(self.stepper(
                        "schedule-from",
                        minute_label(from),
                        move |this, _, cx| {
                            this.update_condition(
                                group,
                                index,
                                |condition| shift_schedule(condition, true, -30),
                                cx,
                            )
                        },
                        move |this, _, cx| {
                            this.update_condition(
                                group,
                                index,
                                |condition| shift_schedule(condition, true, 30),
                                cx,
                            )
                        },
                        cx,
                    ))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(c.schedule_to),
                    )
                    .child(self.stepper(
                        "schedule-to",
                        minute_label(to),
                        move |this, _, cx| {
                            this.update_condition(
                                group,
                                index,
                                |condition| shift_schedule(condition, false, -30),
                                cx,
                            )
                        },
                        move |this, _, cx| {
                            this.update_condition(
                                group,
                                index,
                                |condition| shift_schedule(condition, false, 30),
                                cx,
                            )
                        },
                        cx,
                    )),
            )
            .into_any_element()
    }
}

fn minute_label(minute_of_day: u16) -> String {
    format!("{:02}:{:02}", minute_of_day / 60, minute_of_day % 60)
}

/// Moves one end of a schedule window, wrapping at midnight rather than
/// clamping, because a window that crosses midnight is legal.
fn shift_schedule(condition: &mut Condition, start: bool, delta: i32) {
    let Condition::TimeSchedule { schedule } = condition else {
        return;
    };
    let day = i32::from(awake_core::MINUTES_PER_DAY);
    let current = if start {
        i32::from(schedule.from_minute_of_day)
    } else {
        i32::from(schedule.to_minute_of_day)
    };
    let next = (current + delta).rem_euclid(day) as u16;
    let candidate = if start {
        Schedule::new(schedule.days.clone(), next, schedule.to_minute_of_day)
    } else {
        Schedule::new(schedule.days.clone(), schedule.from_minute_of_day, next)
    };
    if let Ok(candidate) = candidate {
        *schedule = candidate;
    }
}

fn weekday_label(day: Weekday, c: &'static Copy) -> &'static str {
    match day {
        Weekday::Monday => c.weekday_monday,
        Weekday::Tuesday => c.weekday_tuesday,
        Weekday::Wednesday => c.weekday_wednesday,
        Weekday::Thursday => c.weekday_thursday,
        Weekday::Friday => c.weekday_friday,
        Weekday::Saturday => c.weekday_saturday,
        Weekday::Sunday => c.weekday_sunday,
    }
}

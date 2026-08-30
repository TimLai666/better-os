//! The Defaults screens.
//!
//! These draw what `defaults_model` decided. No screen here asks the system
//! anything, and no button here changes anything: the two top-level actions and
//! both per-component actions all open a review screen first, which is the
//! whole point of the feature.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    sidebar::SidebarMenuItem,
    tag::Tag,
    *,
};

use better_core::ComponentId;
use defaults_core::{AggregateState, PlanKind, Selection};

use crate::{
    app::ManagerApp,
    defaults_app::now_seconds,
    defaults_model::{
        DefaultsRow, DefaultsSummary, PrimaryAction, ResultTone, ReviewComponent, ReviewEntry,
        SecondaryAction, aggregate_label, component_rows, integration_state_label, kind_label,
        outcome_headline, relative_time, result_rows, session_effect_label, skip_reason_label,
        warning_label,
    },
    i18n::copy,
    model::Page,
};

impl ManagerApp {
    pub(crate) fn defaults_nav_item(&self, cx: &mut Context<Self>) -> SidebarMenuItem {
        let c = copy(self.locale);
        SidebarMenuItem::new(c.defaults)
            .icon(IconName::Star)
            .active(self.page_is_active(&Page::Defaults))
            .on_click(cx.listener(|this, _, _, cx| {
                this.open_defaults(cx);
            }))
    }

    /// The rows the Defaults screen and the component detail both read.
    fn defaults_rows(&self) -> Vec<DefaultsRow> {
        let Some(report) = self.defaults.report.as_ref() else {
            return Vec::new();
        };
        let names = self.defaults_names();
        let icons: std::collections::BTreeMap<ComponentId, better_core::ComponentIcon> = self
            .manager
            .manifests()
            .map(|manifest| (manifest.id.clone(), manifest.icon))
            .collect();
        component_rows(
            self.locale,
            report,
            &self.defaults.verified,
            &|component| crate::defaults_app::component_name(&names, component),
            &|component| {
                icons
                    .get(component)
                    .copied()
                    .unwrap_or(better_core::ComponentIcon::Generic)
            },
        )
    }

    fn defaults_state_tag(&self, aggregate: &AggregateState) -> Tag {
        let label = aggregate_label(self.locale, aggregate);
        match aggregate {
            AggregateState::Default => Tag::success().small().rounded_full().child(label),
            AggregateState::NotDefault => Tag::secondary().small().rounded_full().child(label),
            AggregateState::PartiallyDefault => Tag::info().small().rounded_full().child(label),
            AggregateState::ChangedExternally => Tag::warning().small().rounded_full().child(label),
            AggregateState::NeedsSignOut => {
                Tag::info().outline().small().rounded_full().child(label)
            }
            AggregateState::Conflict { .. } => Tag::danger().small().rounded_full().child(label),
            AggregateState::Unavailable { .. } => Tag::secondary()
                .outline()
                .small()
                .rounded_full()
                .child(label),
            AggregateState::Unknown { .. } => {
                Tag::warning().outline().small().rounded_full().child(label)
            }
        }
    }

    pub(crate) fn defaults_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let rows = self.defaults_rows();
        let summary = self
            .defaults
            .report
            .as_ref()
            .map(DefaultsSummary::of)
            .unwrap_or_default();

        v_flex()
            .gap_5()
            .when_some(self.error_banner(cx), |view, error| view.child(error))
            .child(self.page_heading(c.defaults_title, c.defaults_subtitle, false, compact))
            .when(self.defaults.failed, |view| {
                view.child(
                    self.surface(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(c.defaults_read_failed),
                        cx,
                    ),
                )
            })
            .when(self.defaults.busy, |view| {
                view.child(
                    self.surface(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(c.defaults_working),
                        cx,
                    ),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_4()
                    .items_stretch()
                    .flex_wrap()
                    .child(self.metric_card(
                        c.defaults_are_default,
                        format!("{} / {}", summary.are_default, summary.total),
                        c.components,
                        IconName::CircleCheck,
                        cx,
                    ))
                    .child(self.metric_card(
                        c.defaults_can_change,
                        summary.can_change.to_string(),
                        c.components,
                        IconName::ArrowDown,
                        cx,
                    ))
                    .child(self.metric_card(
                        c.defaults_changed_externally,
                        summary.changed_externally.to_string(),
                        c.components,
                        IconName::Info,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_3()
                    .items_center()
                    .flex_wrap()
                    .child(
                        Button::new("defaults-apply-all")
                            .primary()
                            .label(c.use_better_defaults)
                            .disabled(rows.is_empty() || self.defaults.busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.review_defaults(PlanKind::Apply, Selection::All, cx);
                            })),
                    )
                    .child(
                        Button::new("defaults-restore-all")
                            .label(c.restore_previous_defaults)
                            .disabled(rows.is_empty() || self.defaults.busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.review_defaults(PlanKind::Restore, Selection::All, cx);
                            })),
                    )
                    .child(
                        Button::new("defaults-verify-all")
                            .label(c.verify_again)
                            .disabled(rows.is_empty() || self.defaults.busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.verify_defaults(Selection::All, cx);
                            })),
                    ),
            )
            .when(rows.is_empty() && !self.defaults.busy, |view| {
                view.child(
                    self.empty_state(
                        IconName::Info,
                        c.defaults_empty_title,
                        c.defaults_empty_detail,
                        Button::new("defaults-empty-components")
                            .label(c.components)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate(Page::Components, cx);
                            })),
                        cx,
                    ),
                )
            })
            .children(
                rows.iter()
                    .map(|row| self.defaults_card(row, compact, cx))
                    .collect::<Vec<_>>(),
            )
            .into_any_element()
    }

    fn defaults_card(
        &self,
        row: &DefaultsRow,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let kinds = row
            .kinds
            .iter()
            .map(|kind| kind_label(self.locale, *kind))
            .collect::<Vec<_>>()
            .join(" · ");
        self.surface(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_3()
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_4()
                        .items_center()
                        .justify_between()
                        .flex_wrap()
                        .child(
                            h_flex()
                                .min_w(px(if compact { 220.0 } else { 320.0 }))
                                .flex_1()
                                .gap_3()
                                .items_center()
                                .child(
                                    div()
                                        .size_10()
                                        .flex_shrink_0()
                                        .rounded(cx.theme().radius)
                                        .bg(cx.theme().secondary)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(Icon::new(self.component_icon(row.icon)).small()),
                                )
                                .child(
                                    v_flex()
                                        .min_w_0()
                                        .gap_1()
                                        .child(div().font_semibold().child(row.name.clone()))
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(kinds),
                                        ),
                                ),
                        )
                        .child(
                            h_flex()
                                .min_w_0()
                                .gap_2()
                                .items_center()
                                .flex_wrap()
                                .child(self.defaults_state_tag(&row.aggregate))
                                .child(Tag::secondary().small().rounded_full().child(
                                    if row.restore_available {
                                        c.saved_previous_value
                                    } else {
                                        c.no_saved_previous_value
                                    },
                                )),
                        ),
                )
                .child(self.key_value_row(c.current_owner, row.current_owner.clone(), cx))
                .child(self.key_value_row(c.better_target, row.target_owner.clone(), cx))
                .child(self.key_value_row(
                    c.last_verified,
                    relative_time(self.locale, row.last_verified, now_seconds()),
                    cx,
                ))
                .child(self.defaults_row_actions(row, cx)),
            cx,
        )
    }

    fn defaults_row_actions(&self, row: &DefaultsRow, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let busy = self.defaults.busy;
        let primary_id = row.component.clone();
        let primary = match row.primary {
            PrimaryAction::MakeDefault => Button::new(row.element_id("defaults-primary"))
                .primary()
                .label(row.primary.label(self.locale))
                .disabled(busy)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.review_defaults(PlanKind::Apply, Selection::one(primary_id.clone()), cx);
                })),
            PrimaryAction::AlreadyDefault => Button::new(row.element_id("defaults-primary"))
                .label(row.primary.label(self.locale))
                .disabled(true),
            PrimaryAction::Verify => Button::new(row.element_id("defaults-primary"))
                .label(row.primary.label(self.locale))
                .disabled(busy)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.verify_defaults(Selection::one(primary_id.clone()), cx);
                })),
        };

        let mut actions = h_flex().gap_2().items_center().flex_wrap().child(primary);
        for secondary in &row.secondary {
            let component = row.component.clone();
            actions = match secondary {
                SecondaryAction::ReviewChanges => actions.child(
                    Button::new(row.element_id("defaults-review"))
                        .label(c.review_changes)
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.review_defaults(
                                PlanKind::Apply,
                                Selection::one(component.clone()),
                                cx,
                            );
                        })),
                ),
                SecondaryAction::RestorePreviousDefault => actions.child(
                    Button::new(row.element_id("defaults-restore"))
                        .label(c.restore_previous_defaults)
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.review_defaults(
                                PlanKind::Restore,
                                Selection::one(component.clone()),
                                cx,
                            );
                        })),
                ),
                SecondaryAction::VerifyAgain => actions.child(
                    Button::new(row.element_id("defaults-verify"))
                        .label(c.verify_again)
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.verify_defaults(Selection::one(component.clone()), cx);
                        })),
                ),
            };
        }
        let detail_id = row.component.clone();
        actions
            .child(
                Button::new(row.element_id("defaults-details"))
                    .label(c.details)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_defaults_component(&detail_id, cx);
                    })),
            )
            .into_any_element()
    }

    pub(crate) fn defaults_component_page(
        &self,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let Page::DefaultsComponent(component) = &self.page else {
            return self.defaults_page(compact, cx);
        };
        let rows = self.defaults_rows();
        let Some(row) = rows.iter().find(|row| &row.component == component) else {
            return self.empty_state(
                IconName::Info,
                c.defaults_empty_title,
                c.defaults_empty_detail,
                Button::new("defaults-detail-back")
                    .label(c.back)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.navigate(Page::Defaults, cx);
                    })),
                cx,
            );
        };

        v_flex()
            .gap_5()
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_3()
                    .items_start()
                    .justify_between()
                    .flex_wrap()
                    .child(
                        v_flex()
                            .min_w(px(240.0))
                            .flex_1()
                            .gap_2()
                            .child(div().text_2xl().font_bold().child(row.name.clone()))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .flex_wrap()
                                    .child(self.defaults_state_tag(&row.aggregate)),
                            ),
                    )
                    .child(
                        Button::new("defaults-component-back")
                            .label(c.back)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate(Page::Defaults, cx);
                            })),
                    ),
            )
            .child(self.defaults_row_actions(row, cx))
            .child(div().text_lg().font_semibold().child(c.integrations_label))
            .children(
                row.integrations
                    .iter()
                    .map(|integration| {
                        self.surface(
                            v_flex()
                                .gap_1()
                                .child(
                                    h_flex()
                                        .w_full()
                                        .min_w_0()
                                        .gap_3()
                                        .items_center()
                                        .justify_between()
                                        .flex_wrap()
                                        .child(
                                            div()
                                                .font_semibold()
                                                .child(kind_label(self.locale, integration.kind)),
                                        )
                                        .child(Tag::secondary().small().rounded_full().child(
                                            integration_state_label(
                                                self.locale,
                                                &integration.state,
                                            ),
                                        )),
                                )
                                .child(self.key_value_row(
                                    c.current_owner,
                                    integration.current_owner.clone(),
                                    cx,
                                ))
                                .child(self.key_value_row(
                                    c.better_target,
                                    integration.target_owner.clone(),
                                    cx,
                                ))
                                .child(self.key_value_row(
                                    c.restart_requirement,
                                    session_effect_label(self.locale, integration.session_effect),
                                    cx,
                                ))
                                .child(self.key_value_row(
                                    c.snapshot_label,
                                    if integration.restore_available {
                                        c.saved_previous_value
                                    } else {
                                        c.no_saved_previous_value
                                    },
                                    cx,
                                ))
                                .child(self.key_value_row(
                                    c.last_verified,
                                    relative_time(
                                        self.locale,
                                        integration.last_verified,
                                        now_seconds(),
                                    ),
                                    cx,
                                )),
                            cx,
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .into_any_element()
    }

    pub(crate) fn defaults_review_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let Some(review) = self.defaults.review.as_ref() else {
            return self.empty_state(
                IconName::Info,
                c.nothing_to_change,
                c.defaults_subtitle,
                Button::new("defaults-review-back")
                    .label(c.back)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cancel_defaults_review(cx);
                    })),
                cx,
            );
        };
        let restoring = review.kind() == PlanKind::Restore;
        let summary = review.summary();
        let elevation = review.elevation();
        let approval = review.approve().is_some();

        v_flex()
            .gap_5()
            .child(self.page_heading(
                if restoring {
                    c.restore_review_title
                } else {
                    c.defaults_review_title
                },
                if restoring {
                    c.restore_review_subtitle
                } else {
                    c.defaults_review_subtitle
                },
                false,
                compact,
            ))
            .children(
                review
                    .components()
                    .iter()
                    .map(|component| self.defaults_review_component(component, restoring, cx))
                    .collect::<Vec<_>>(),
            )
            .child(
                self.surface(
                    v_flex()
                        .gap_1()
                        .child(self.key_value_row(
                            c.components_selected,
                            summary.components_selected.to_string(),
                            cx,
                        ))
                        .child(self.key_value_row(
                            c.settings_affected,
                            summary.settings_affected.to_string(),
                            cx,
                        ))
                        .child(self.key_value_row(
                            c.state_needs_sign_out,
                            summary.needs_sign_out.to_string(),
                            cx,
                        ))
                        .child(self.key_value_row(
                            c.effect_restart,
                            summary.needs_restart.to_string(),
                            cx,
                        ))
                        .child(self.key_value_row(
                            c.snapshot_label,
                            if summary.will_capture > 0 {
                                c.snapshot_will_capture
                            } else {
                                c.snapshot_nothing_to_capture
                            },
                            cx,
                        ))
                        .child(self.key_value_row(
                            c.manual_action_required,
                            summary.manual_actions.to_string(),
                            cx,
                        ))
                        .child(self.key_value_row(
                            c.awaiting_confirmation,
                            summary.awaiting_confirmation.to_string(),
                            cx,
                        ))
                        .when(summary.damaged_snapshots > 0, |view| {
                            view.child(self.key_value_row(
                                c.damaged_snapshots,
                                summary.damaged_snapshots.to_string(),
                                cx,
                            ))
                        })
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(if elevation.requested {
                                    c.skip_requires_administrator
                                } else {
                                    c.no_elevated_access
                                }),
                        )
                        .when(elevation.excluded_needing_administrator > 0, |view| {
                            view.child(self.key_value_row(
                                c.elevated_excluded,
                                elevation.excluded_needing_administrator.to_string(),
                                cx,
                            ))
                        }),
                    cx,
                ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .justify_end()
                    .flex_wrap()
                    .when(!approval, |row| {
                        row.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.nothing_to_change),
                        )
                    })
                    .child(
                        Button::new("defaults-review-cancel")
                            .label(c.cancel)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cancel_defaults_review(cx);
                            })),
                    )
                    .child(
                        Button::new("defaults-review-apply")
                            .primary()
                            .label(if restoring {
                                c.restore_selected_defaults
                            } else {
                                c.apply_selected_defaults
                            })
                            .disabled(!approval || self.defaults.busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.apply_defaults_review(cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn defaults_review_component(
        &self,
        component: &ReviewComponent,
        restoring: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let id = component.component.clone();
        self.surface(
            v_flex()
                .gap_3()
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_3()
                        .items_center()
                        .justify_between()
                        .flex_wrap()
                        .child(
                            Checkbox::new(SharedString::from(format!(
                                "defaults-select-{}",
                                component.component
                            )))
                            .label(component.name.clone())
                            .checked(component.selected)
                            .on_click(cx.listener(
                                move |this, _: &bool, _, cx| {
                                    this.toggle_defaults_component(&id, cx);
                                },
                            )),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{} {}", component.changes, c.settings_affected)),
                        ),
                )
                .children(
                    component
                        .entries
                        .iter()
                        .map(|entry| self.defaults_review_entry(entry, restoring, cx))
                        .collect::<Vec<_>>(),
                ),
            cx,
        )
    }

    fn defaults_review_entry(
        &self,
        entry: &ReviewEntry,
        restoring: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let component = entry.component.clone();
        let integration = entry.integration.clone();
        let confirmed =
            entry.confirmed
                || self.defaults.review.as_ref().is_some_and(|review| {
                    review.is_confirmed(&entry.component, &entry.integration)
                });
        v_flex()
            .w_full()
            .min_w_0()
            .gap_1()
            .py_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_3()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .child(
                        div()
                            .min_w_0()
                            .font_medium()
                            .child(kind_label(self.locale, entry.kind)),
                    )
                    .when(restoring, |row| {
                        row.child(
                            Tag::secondary()
                                .small()
                                .rounded_full()
                                .child(entry.restore_class.label(self.locale)),
                        )
                    }),
            )
            .child(self.key_value_row(c.current_value, entry.current_owner.clone(), cx))
            .child(self.key_value_row(c.new_value, entry.new_owner.clone(), cx))
            .child(self.key_value_row(
                c.restart_requirement,
                session_effect_label(self.locale, entry.session_effect),
                cx,
            ))
            .child(self.key_value_row(
                c.snapshot_label,
                if entry.restorable {
                    c.can_be_restored
                } else {
                    c.cannot_be_restored
                },
                cx,
            ))
            .when_some(entry.skipped.as_ref(), |view, reason| {
                view.child(self.key_value_row(
                    c.nothing_to_change,
                    skip_reason_label(self.locale, reason),
                    cx,
                ))
            })
            .children(
                entry
                    .warnings
                    .iter()
                    .map(|warning| {
                        div()
                            .text_sm()
                            .text_color(cx.theme().warning)
                            .child(warning_label(self.locale, warning))
                            .into_any_element()
                    })
                    .collect::<Vec<_>>(),
            )
            .when(entry.requires_confirmation, |view| {
                view.child(
                    Checkbox::new(SharedString::from(entry.element_id("defaults-confirm")))
                        .label(c.confirm_overwrite)
                        .checked(confirmed)
                        .on_click(cx.listener(move |this, _: &bool, _, cx| {
                            this.toggle_defaults_confirmation(&component, &integration, cx);
                        })),
                )
            })
            .into_any_element()
    }

    pub(crate) fn defaults_results_page(
        &self,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let Some(outcome) = self.defaults.outcome.as_ref() else {
            return self.defaults_page(compact, cx);
        };
        let names = self.defaults_names();
        let rows = result_rows(self.locale, outcome, &|component| {
            crate::defaults_app::component_name(&names, component)
        });

        v_flex()
            .gap_5()
            .child(self.page_heading(
                c.defaults_results_title,
                c.defaults_results_subtitle,
                false,
                compact,
            ))
            .child(
                self.surface(
                    h_flex().gap_2().items_center().child(
                        Tag::secondary()
                            .small()
                            .rounded_full()
                            .child(outcome_headline(self.locale, outcome)),
                    ),
                    cx,
                ),
            )
            .child(
                self.surface(
                    v_flex().gap_2().children(
                        rows.iter()
                            .map(|row| {
                                let tag = match row.tone {
                                    ResultTone::Success => Tag::success(),
                                    ResultTone::Pending => Tag::info(),
                                    ResultTone::Warning => Tag::warning(),
                                    ResultTone::Failure => Tag::danger(),
                                    ResultTone::Neutral => Tag::secondary(),
                                };
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .gap_3()
                                    .items_center()
                                    .justify_between()
                                    .flex_wrap()
                                    .py_2()
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        v_flex()
                                            .min_w(px(220.0))
                                            .flex_1()
                                            .gap_1()
                                            .child(div().font_medium().child(row.name.clone()))
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(
                                                        row.detail
                                                            .clone()
                                                            .unwrap_or_else(|| c.none.to_string()),
                                                    ),
                                            ),
                                    )
                                    .child(tag.small().rounded_full().child(row.label))
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>(),
                    ),
                    cx,
                ),
            )
            .child(
                h_flex().w_full().gap_3().justify_end().child(
                    Button::new("defaults-results-back")
                        .primary()
                        .label(c.back)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.navigate(Page::Defaults, cx);
                        })),
                ),
            )
            .into_any_element()
    }
}

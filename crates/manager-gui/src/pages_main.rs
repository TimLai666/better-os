use better_ui::fluent::tabs::{SelectorBar, SelectorItem};
use better_ui::{StatusPill, StatusTone};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    *,
};
use manager_core::catalog::now_unix_seconds;
use manager_core::{
    ActivityKind, ComponentFilterPreference, ComponentStatus, DesiredOperation, is_self_component,
};

use crate::{
    app::ManagerApp,
    i18n::copy,
    model::{
        ActivityFilter, CatalogLine, ComponentInfo, DetailTab, Page, UpdateCheck,
        unorderable_detail,
    },
};

impl ManagerApp {
    pub(crate) fn components_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let query = self.search_query.trim().to_ascii_lowercase();
        let filter = self.state.settings.component_filter;
        let visible = self
            .components()
            .into_iter()
            .filter(|component| self.matches_component_filter(component, filter))
            .filter(|component| {
                query.is_empty()
                    || component.name.to_ascii_lowercase().contains(&query)
                    || component.summary.to_ascii_lowercase().contains(&query)
                    || component.core_id.as_str().contains(&query)
            })
            .collect::<Vec<_>>();

        v_flex()
            .gap_5()
            .when_some(self.error_banner(cx), |view, error| view.child(error))
            .child(self.page_heading(c.components_title, c.components_subtitle, true, compact))
            .child(self.catalog_status_row(cx))
            // Views of one list: a selector bar, not a row of buttons.
            .child(
                div()
                    .min_w_0()
                    .overflow_x_hidden()
                    .child(self.view_switcher(
                        "component-filter",
                        &[
                            (ComponentFilterPreference::All, c.all_activity),
                            (ComponentFilterPreference::Installed, c.installed),
                            (ComponentFilterPreference::Updates, c.updates),
                            (ComponentFilterPreference::Disabled, c.disabled),
                            (ComponentFilterPreference::NeedsAttention, c.degraded),
                        ],
                        self.state.settings.component_filter,
                        |this, filter, cx| this.set_component_filter(filter, cx),
                        cx,
                    )),
            )
            .when(visible.is_empty(), |view| {
                view.child(
                    self.empty_state(
                        IconName::Search,
                        c.no_matches,
                        c.components_subtitle,
                        Button::new("clear-search")
                            .label(c.clear_search)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.clear_search(window, cx);
                            })),
                        cx,
                    ),
                )
            })
            .when(!visible.is_empty(), |view| {
                view.children(
                    visible
                        .iter()
                        .map(|component| self.component_card(component, compact, cx)),
                )
            })
            .into_any_element()
    }

    /// Where this list came from, whether it may be behind the published one,
    /// and the one button that goes and looks.
    ///
    /// A degraded catalog is drawn as a warning rather than a note: the list is
    /// what a user decides to install from, and one that quietly described the
    /// previous release would be worse than one that admits it might.
    fn catalog_status_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let line = CatalogLine::present(self.locale, &self.catalog_status, now_unix_seconds());
        let degraded = line.is_degraded();
        let rejected = line.rejected_line(self.locale);

        self.surface(
            h_flex()
                .min_w_0()
                .gap_3()
                .flex_wrap()
                .items_start()
                .justify_between()
                .child(
                    h_flex()
                        .min_w_0()
                        .gap_3()
                        .items_start()
                        .child(Icon::new(if degraded {
                            IconName::TriangleAlert
                        } else {
                            IconName::Inbox
                        }))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .child(div().font_medium().child(line.age.clone()))
                                .when_some(line.warning, |view, warning| {
                                    view.child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().warning)
                                            .child(warning),
                                    )
                                })
                                .when_some(rejected, |view, rejected| {
                                    view.child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(rejected),
                                    )
                                }),
                        ),
                )
                .child(
                    Button::new("refresh-catalog")
                        .label(if self.catalog_refreshing {
                            c.catalog_refreshing
                        } else {
                            c.catalog_refresh
                        })
                        .disabled(self.catalog_refreshing)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.refresh_catalog(cx);
                        })),
                ),
            cx,
        )
    }

    /// The Updates page's one summary card: what was found, when the version
    /// list was last checked, and the buttons that act on it (ticket 111).
    ///
    /// The count says what it counts ("8 components have new versions"), the
    /// primary button names what it leads to ("Review and update all": the
    /// plan is reviewed before anything runs), and the list's age and any
    /// warning about it sit under the count instead of on a second card that
    /// repeated the check button. The check starts the same catalog refresh
    /// the Apps screen's does, on the same background thread, so the window
    /// keeps drawing while the fetch runs and the two screens can never
    /// disagree about the catalog.
    fn update_summary(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let check = self.update_check();
        let line = CatalogLine::present(self.locale, &self.catalog_status, now_unix_seconds());
        let (title, failure) = match &check {
            UpdateCheck::Running => (c.checking_updates.to_string(), None),
            UpdateCheck::Failed(reason) => (c.update_check_failed.to_string(), Some(*reason)),
            UpdateCheck::Found(sentence) => (sentence.clone(), None),
            UpdateCheck::UpToDate => (c.update_check_up_to_date.to_string(), None),
            UpdateCheck::NoneInCurrentList => (c.update_check_none_in_list.to_string(), None),
        };
        // A failed check already says which list is on screen; saying it a
        // second time as the list's warning would repeat it.
        let warning = failure.or(line.warning);
        let rejected = line.rejected_line(self.locale);
        let has_updates = self.update_plan_count() > 0;
        let checking = self.catalog_refreshing;

        let check_button = Button::new("check-updates")
            .label(if checking {
                c.catalog_refreshing
            } else if has_updates {
                c.check_again
            } else {
                c.check_updates
            })
            .disabled(checking)
            .when(!has_updates, |button| button.primary())
            .on_click(cx.listener(|this, _, _, cx| {
                this.check_for_updates(cx);
            }));

        self.surface(
            h_flex()
                .min_w_0()
                .gap_3()
                .flex_wrap()
                .items_start()
                .justify_between()
                .child(
                    h_flex()
                        .min_w(px(220.0))
                        .flex_1()
                        .gap_3()
                        .items_start()
                        .child(Icon::new(if warning.is_some() {
                            IconName::TriangleAlert
                        } else if has_updates {
                            IconName::ArrowDown
                        } else {
                            IconName::Check
                        }))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .child(div().text_lg().font_semibold().child(title))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(line.age.clone()),
                                )
                                .when_some(warning, |view, warning| {
                                    view.child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().warning)
                                            .child(warning),
                                    )
                                })
                                .when_some(rejected, |view, rejected| {
                                    view.child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(rejected),
                                    )
                                }),
                        ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .items_center()
                        .child(check_button)
                        .when(has_updates, |row| {
                            row.child(
                                Button::new("updates-review")
                                    .primary()
                                    .label(c.review_and_update_all)
                                    .disabled(checking)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.prepare_update_all(cx);
                                    })),
                            )
                        }),
                ),
            cx,
        )
    }

    /// A row that switches what the page lists: the kit's selector bar
    /// (ticket 103), one item per view, the current one marked.
    fn view_switcher<T: Copy + PartialEq + 'static>(
        &self,
        id: &'static str,
        views: &[(T, &'static str)],
        current: T,
        choose: fn(&mut Self, T, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> SelectorBar {
        let values: Vec<T> = views.iter().map(|(value, _)| *value).collect();
        let selected = values.iter().position(|value| *value == current);
        let items = views
            .iter()
            .map(|(_, label)| SelectorItem::new(*label))
            .collect();
        let view = cx.entity().downgrade();
        SelectorBar::new(id, items, selected).on_select(move |index, _, cx| {
            if let Some(value) = values.get(index).copied() {
                let _ = view.update(cx, |this, cx| choose(this, value, cx));
            }
        })
    }

    fn matches_component_filter(
        &self,
        component: &ComponentInfo,
        filter: ComponentFilterPreference,
    ) -> bool {
        match filter {
            ComponentFilterPreference::All => true,
            ComponentFilterPreference::Installed => component.installed_version.is_some(),
            ComponentFilterPreference::Updates => {
                component.state == ComponentStatus::UpdateAvailable
            }
            ComponentFilterPreference::Disabled => component.state == ComponentStatus::Disabled,
            ComponentFilterPreference::NeedsAttention => matches!(
                component.state,
                ComponentStatus::Degraded
                    | ComponentStatus::Failed
                    | ComponentStatus::RestoreAvailable
                    | ComponentStatus::Incompatible
            ),
        }
    }

    pub(crate) fn component_detail_page(
        &self,
        _compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let Some(component) = self.selected_component() else {
            return self.empty_state(
                IconName::Info,
                c.no_matches,
                c.components_subtitle,
                Button::new("back-components")
                    .label(c.back)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.navigate(Page::Components, cx);
                    })),
                cx,
            );
        };
        let pending = self.is_pending(&component.core_id);
        // dpkg holds it at a version nothing can order: say so, and offer no
        // action the planner would refuse (ticket 69).
        let unorderable = self
            .unorderable_check(&component.core_id)
            .and_then(|check| unorderable_detail(self.locale, check));
        let disable_id = component.core_id.clone();
        let verify_id = component.core_id.clone();
        let restore_id = component.core_id.clone();
        let retry_id = component.core_id.clone();
        let remove_id = component.core_id.clone();
        // Removing is offered for anything installed except the manager
        // itself, which `manager-core` refuses to plan.
        let removable = component.installed_version.is_some()
            && !is_self_component(&component.core_id)
            && !pending
            && !matches!(component.state, ComponentStatus::Incompatible);
        let body = match self.detail_tab {
            DetailTab::Overview => self.surface(
                v_flex()
                    .gap_3()
                    .child(div().text_lg().font_semibold().child(c.what_it_does))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.declared_or_not(&component.detail)),
                    )
                    .child(self.key_value_row(
                        c.replaces_label,
                        if component.replaces.is_empty() {
                            c.none.to_string()
                        } else {
                            component.replaces.join(", ")
                        },
                        cx,
                    ))
                    .child(self.key_value_row(
                        c.enhances_label,
                        if component.enhances.is_empty() {
                            c.none.to_string()
                        } else {
                            component.enhances.join(", ")
                        },
                        cx,
                    ))
                    // What happens after the update, and only while one is
                    // waiting: there is nothing to restart about a component
                    // that is not about to change (ticket 111).
                    .when_some(
                        (component.state == ComponentStatus::UpdateAvailable)
                            .then(|| {
                                self.restart_requirement_label(
                                    DesiredOperation::Update,
                                    component.restart_requirement,
                                )
                            })
                            .flatten(),
                        |view, label| {
                            view.child(self.key_value_row(c.restart_requirement, label, cx))
                        },
                    )
                    // Two rows, never one. Folding them into a single "version"
                    // line meant a component that was never installed reported
                    // the catalog's version as its installed version.
                    .child(self.key_value_row(
                        c.current_version,
                        component.installed_label(c.not_installed),
                        cx,
                    ))
                    .child(self.key_value_row(
                        c.available_version,
                        component.available_version.clone(),
                        cx,
                    ))
                    .when(component.installed_outside_manager(), |view| {
                        view.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.installed_outside_manager),
                        )
                    })
                    .child(self.key_value_row(
                        c.health,
                        self.status_tag(component.state, pending, cx),
                        cx,
                    )),
                cx,
            ),
            DetailTab::Versions => self.surface(
                v_flex()
                    .gap_2()
                    .child(self.key_value_row(
                        c.current_version,
                        component.installed_label(c.not_installed),
                        cx,
                    ))
                    .child(self.key_value_row(
                        c.available_version,
                        component.available_version.clone(),
                        cx,
                    ))
                    .when(component.installed_outside_manager(), |view| {
                        view.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.installed_outside_manager),
                        )
                    })
                    .child(self.release_notes_surface(&component, cx)),
                cx,
            ),
            DetailTab::Permissions => self.surface(
                v_flex()
                    .gap_3()
                    .child(div().text_lg().font_semibold().child(c.files_touched))
                    .children(if component.paths.is_empty() {
                        vec![self.key_value_row(c.files_touched, c.none, cx)]
                    } else {
                        component
                            .paths
                            .iter()
                            .cloned()
                            .map(|path| self.key_value_row(c.files_touched, path, cx))
                            .collect()
                    })
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(c.no_privileged_actions),
                    ),
                cx,
            ),
        };

        v_flex()
            .gap_5()
            .when_some(self.error_banner(cx), |view, error| view.child(error))
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
                            .child(div().text_2xl().font_bold().child(component.name.clone()))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .flex_wrap()
                                    .child(self.kind_tag(component.kind, cx))
                                    .child(self.status_tag(component.state, pending, cx)),
                            ),
                    )
                    .child(
                        Button::new("detail-back")
                            .label(c.back)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate(Page::Components, cx);
                            })),
                    ),
            )
            .child(self.view_switcher(
                "detail-tabs",
                &[
                    (DetailTab::Overview, c.overview_tab),
                    (DetailTab::Versions, c.versions_tab),
                    (DetailTab::Permissions, c.files_touched),
                ],
                self.detail_tab,
                |this, tab, cx| {
                    this.detail_tab = tab;
                    cx.notify();
                },
                cx,
            ))
            // A blocking drift is the reason nothing else on this page will
            // work, so it goes above the failure that may have caused it. Until
            // ticket 49 the window said nothing at all about drift: the actions
            // simply refused and the Updates screen offered nothing.
            .when_some(component.drift_notice(self.locale), |view, notice| {
                view.child(self.drift_card(&component, &notice, cx))
            })
            // A recorded failure belongs on the page that shows the component,
            // not three screens away behind a red tag with no words on it.
            .when_some(unorderable.clone(), |view, detail| {
                view.child(
                    self.surface(
                        v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_lg()
                                    .font_semibold()
                                    .child(c.check_unorderable_version),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(detail),
                            ),
                        cx,
                    ),
                )
            })
            .when_some(component.failure.clone(), |view, failure| {
                view.child(self.failure_card(c.last_install_failed.to_string(), &failure, None, cx))
            })
            .child(body)
            .child(
                h_flex()
                    .gap_3()
                    .flex_wrap()
                    .when(
                        unorderable.is_none()
                            && matches!(
                                component.state,
                                ComponentStatus::Available
                                    | ComponentStatus::UpdateAvailable
                                    | ComponentStatus::Disabled
                            ),
                        |row| row.child(self.component_action_button(&component, cx)),
                    )
                    .when(
                        component.installed_version.is_some()
                            && component.enabled
                            && !pending
                            && !matches!(component.state, ComponentStatus::Incompatible),
                        |row| {
                            row.child(Button::new("detail-disable").label(c.disable).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.prepare_component_operation(
                                        &disable_id,
                                        DesiredOperation::Disable,
                                        cx,
                                    );
                                }),
                            ))
                        },
                    )
                    .when(
                        component.installed_version.is_some()
                            && !pending
                            && !matches!(component.state, ComponentStatus::Incompatible),
                        |row| {
                            row.child(Button::new("detail-verify").label(c.retry_check).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.prepare_component_operation(
                                        &verify_id,
                                        DesiredOperation::Verify,
                                        cx,
                                    );
                                }),
                            ))
                        },
                    )
                    .when(
                        component.state == ComponentStatus::RestoreAvailable,
                        |row| {
                            row.child(
                                Button::new("detail-restore")
                                    .danger()
                                    .label(c.restore_previous)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.prepare_component_operation(
                                            &restore_id,
                                            DesiredOperation::Restore,
                                            cx,
                                        );
                                    })),
                            )
                        },
                    )
                    // What "try again" is depends on what actually happened. An
                    // install that failed before anything was applied leaves
                    // nothing installed, and offering a health check there is
                    // an action the planner refuses.
                    .when(
                        unorderable.is_none()
                            && component.failure.is_some()
                            && component.installed_version.is_none()
                            && !pending
                            && !matches!(component.state, ComponentStatus::Incompatible),
                        |row| {
                            row.child(
                                Button::new("detail-retry-install")
                                    .primary()
                                    .label(c.try_install_again)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.prepare_component_operation(
                                            &retry_id,
                                            DesiredOperation::Install,
                                            cx,
                                        );
                                    })),
                            )
                        },
                    )
                    .when(removable, |row| {
                        row.child(
                            Button::new("detail-remove")
                                .danger()
                                .outline()
                                .label(c.remove_component)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.prepare_component_operation(
                                        &remove_id,
                                        DesiredOperation::Remove,
                                        cx,
                                    );
                                })),
                        )
                    }),
            )
            // The manager is a component in its own catalog, and the one row
            // whose removal it cannot carry out. Saying why beats an action
            // that is simply absent.
            .when(
                component.installed_version.is_some() && is_self_component(&component.core_id),
                |view| {
                    view.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(c.cannot_remove_self),
                    )
                },
            )
            .into_any_element()
    }

    pub(crate) fn updates_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let updates = self
            .components()
            .into_iter()
            .filter(|component| component.state == ComponentStatus::UpdateAvailable)
            .collect::<Vec<_>>();
        v_flex()
            .gap_5()
            .when_some(self.error_banner(cx), |view, error| view.child(error))
            .child(self.page_heading(c.updates_title, c.updates_subtitle, false, compact))
            .child(self.update_summary(cx))
            .children(updates.iter().map(|component| {
                v_flex()
                    .gap_2()
                    .child(self.component_card(component, compact, cx))
                    .child(self.release_notes_surface(component, cx))
            }))
            .into_any_element()
    }

    pub(crate) fn activity_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let entries = self
            .state
            .activity
            .iter()
            .rev()
            .filter(|entry| self.activity_matches(entry))
            .cloned()
            .collect::<Vec<_>>();
        v_flex()
            .gap_5()
            .child(self.page_heading(c.activity_title, c.activity_subtitle, false, compact))
            .when_some(self.error_banner(cx), |view, error| view.child(error))
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_3()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .child(self.view_switcher(
                        "activity-filter",
                        &[
                            (ActivityFilter::All, c.all_activity),
                            (ActivityFilter::Success, c.successful),
                            (ActivityFilter::Warning, c.warnings),
                            (ActivityFilter::Failure, c.failures),
                        ],
                        self.activity_filter,
                        |this, filter, cx| {
                            this.activity_filter = filter;
                            cx.notify();
                        },
                        cx,
                    ))
                    .child(
                        Button::new("activity-clear")
                            .label(c.clear_history)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clear_activity(cx);
                            })),
                    ),
            )
            .child(if entries.is_empty() {
                self.empty_state(
                    IconName::Inspector,
                    c.no_activity_title,
                    c.no_activity_subtitle,
                    Button::new("activity-components")
                        .label(c.components)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.navigate(Page::Components, cx);
                        })),
                    cx,
                )
            } else {
                self.surface(
                    v_flex()
                        .gap_2()
                        .children(entries.iter().map(|entry| self.activity_row(entry, cx))),
                    cx,
                )
            })
            .into_any_element()
    }

    /// How one activity entry reads. A record of something that already
    /// happened is never an action, so it is a pill and not a button.
    pub(crate) fn activity_pill(c: &'static crate::i18n::Copy, kind: ActivityKind) -> StatusPill {
        let (label, tone) = match kind {
            ActivityKind::Success | ActivityKind::RecoverySuccess => {
                (c.successful, StatusTone::Success)
            }
            ActivityKind::Failure => (c.failed, StatusTone::Danger),
            ActivityKind::Warning
            | ActivityKind::RecoveryPartial
            | ActivityKind::ManualRecovery => (c.warnings, StatusTone::Warning),
            ActivityKind::Information => (c.activity, StatusTone::Neutral),
        };
        StatusPill::new(label, tone)
    }

    fn activity_row(
        &self,
        entry: &manager_core::ActivityRecord,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let tag = self.pill(Self::activity_pill(c, entry.kind), cx);
        let operation = entry
            .operation
            .map(|operation| self.operation_label(operation))
            .unwrap_or(c.none);
        // A unit entry carries no detail the row could show, so the unit's
        // name travels in the sentence itself.
        let detail = entry
            .stage
            .map(|stage| self.stage_label(stage).to_string())
            .unwrap_or_else(|| {
                crate::app::activity_evidence_text(
                    self.evidence_label(entry.evidence.as_deref(), None),
                    entry.evidence.as_deref(),
                )
            });
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
                    .child(div().font_medium().child(format!(
                        "{} · {}",
                        self.component_name_for_core(entry.component.as_ref()),
                        operation
                    )))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(detail),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(tag)
                    .child(format!("#{}", entry.sequence)),
            )
            .into_any_element()
    }
}

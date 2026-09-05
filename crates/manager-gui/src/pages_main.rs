use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    scroll::ScrollableElement,
    tag::Tag,
    *,
};
use manager_core::catalog::now_unix_seconds;
use manager_core::{ActivityKind, ComponentFilterPreference, ComponentStatus, DesiredOperation};

use crate::{
    app::ManagerApp,
    i18n::copy,
    model::{ActivityFilter, CatalogLine, ComponentInfo, DetailTab, Page},
};

impl ManagerApp {
    pub(crate) fn first_run_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let profile = self.manager.profile();
        v_flex()
            .size_full()
            .min_w_0()
            .bg(cx.theme().secondary)
            .child(
                h_flex()
                    .min_h(px(72.0))
                    .px_6()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .child(Icon::new(IconName::GalleryVerticalEnd))
                    .child(
                        v_flex()
                            .child(div().font_semibold().child(c.brand_name))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(c.manager),
                            ),
                    ),
            )
            .when_some(self.error_banner(cx), |view, error| {
                view.child(div().px_5().pt_5().child(error))
            })
            .child(
                div().flex_1().min_h_0().overflow_y_scrollbar().p_5().child(
                    div().w_full().flex().justify_center().child(
                        v_flex()
                            .w_full()
                            .max_w(px(1040.0))
                            .gap_5()
                            .child(self.page_heading(
                                c.setup_title,
                                c.setup_subtitle,
                                false,
                                compact,
                            ))
                            .child(
                                self.surface(
                                    v_flex()
                                        .gap_5()
                                        .child(div().text_2xl().font_bold().child(c.welcome_title))
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(c.welcome_description),
                                        )
                                        .child(
                                            h_flex()
                                                .min_w_0()
                                                .gap_5()
                                                .items_start()
                                                .flex_wrap()
                                                .child(
                                                    v_flex()
                                                        .min_w(px(280.0))
                                                        .flex_1()
                                                        .gap_3()
                                                        .child(self.bullet_row(
                                                            IconName::CircleCheck,
                                                            c.check_system,
                                                            c.check_system_description,
                                                            cx,
                                                        ))
                                                        .child(self.bullet_row(
                                                            IconName::Inbox,
                                                            c.load_catalog,
                                                            c.load_catalog_description,
                                                            cx,
                                                        )),
                                                )
                                                .child(
                                                    div().min_w(px(280.0)).flex_1().child(
                                                        self.surface(
                                                            v_flex()
                                                                .gap_2()
                                                                .child(
                                                                    div()
                                                                        .text_lg()
                                                                        .font_semibold()
                                                                        .child(
                                                                            c.compatibility_check,
                                                                        ),
                                                                )
                                                                .child(self.key_value_row(
                                                                    c.distribution,
                                                                    format!(
                                                                        "{} {}",
                                                                        profile.distribution,
                                                                        profile.release
                                                                    ),
                                                                    cx,
                                                                ))
                                                                .child(self.key_value_row(
                                                                    c.architecture,
                                                                    profile.architecture.clone(),
                                                                    cx,
                                                                ))
                                                                .child(
                                                                    self.key_value_row(
                                                                        c.components,
                                                                        self.manager
                                                                            .manifests()
                                                                            .count()
                                                                            .to_string(),
                                                                        cx,
                                                                    ),
                                                                ),
                                                            cx,
                                                        ),
                                                    ),
                                                ),
                                        )
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .gap_3()
                                                .items_center()
                                                .justify_end()
                                                .flex_wrap()
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(c.no_changes_yet),
                                                )
                                                .child(
                                                    Button::new("finish-first-run")
                                                        .primary()
                                                        .label(c.continue_label)
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.complete_onboarding(cx);
                                                        })),
                                                ),
                                        ),
                                    cx,
                                ),
                            ),
                    ),
                ),
            )
            .into_any_element()
    }

    pub(crate) fn overview_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let recent = self
            .state
            .activity
            .iter()
            .rev()
            .take(5)
            .cloned()
            .collect::<Vec<_>>();
        let title = if self.current_failure().is_some() {
            c.one_check_failed
        } else {
            c.system_healthy
        };
        v_flex()
            .gap_6()
            .when_some(self.error_banner(cx), |view, error| view.child(error))
            .child(self.page_heading(title, c.component_summary, false, compact))
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_4()
                    .items_stretch()
                    .flex_wrap()
                    .child(self.metric_card(
                        c.installed,
                        self.installed_count().to_string(),
                        c.components,
                        IconName::Inbox,
                        cx,
                    ))
                    .child(self.metric_card(
                        c.updates_ready,
                        self.update_plan_count().to_string(),
                        c.ready_to_review,
                        IconName::ArrowDown,
                        cx,
                    ))
                    .child(self.metric_card(
                        c.health,
                        self.healthy_count().to_string(),
                        c.healthy,
                        IconName::CircleCheck,
                        cx,
                    )),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        self.section_header(
                            c.components,
                            Some(c.component_summary),
                            Some(
                                Button::new("overview-all-components")
                                    .label(c.view)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.navigate(Page::Components, cx);
                                    })),
                            ),
                            cx,
                        ),
                    )
                    .children(
                        self.components()
                            .iter()
                            .map(|component| self.component_card(component, compact, cx)),
                    ),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        self.section_header(
                            c.recent_activity,
                            None,
                            Some(
                                Button::new("overview-all-activity")
                                    .label(c.view_all_activity)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.navigate(Page::Activity, cx);
                                    })),
                            ),
                            cx,
                        ),
                    )
                    .child(if recent.is_empty() {
                        self.empty_state(
                            IconName::Inspector,
                            c.no_activity_title,
                            c.no_activity_subtitle,
                            Button::new("overview-components")
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
                                .children(recent.iter().map(|entry| self.activity_row(entry, cx))),
                            cx,
                        )
                    }),
            )
            .into_any_element()
    }

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
            .child(
                h_flex()
                    .min_w_0()
                    .gap_2()
                    .flex_wrap()
                    .child(self.component_filter_button(
                        "filter-all",
                        ComponentFilterPreference::All,
                        c.all_activity,
                        cx,
                    ))
                    .child(self.component_filter_button(
                        "filter-installed",
                        ComponentFilterPreference::Installed,
                        c.installed,
                        cx,
                    ))
                    .child(self.component_filter_button(
                        "filter-updates",
                        ComponentFilterPreference::Updates,
                        c.updates,
                        cx,
                    ))
                    .child(self.component_filter_button(
                        "filter-disabled",
                        ComponentFilterPreference::Disabled,
                        c.disabled,
                        cx,
                    ))
                    .child(self.component_filter_button(
                        "filter-attention",
                        ComponentFilterPreference::NeedsAttention,
                        c.degraded,
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
                                .min_w_0()
                                .gap_1()
                                .child(
                                    div()
                                        .font_medium()
                                        .child(format!("{} · {}", line.source, line.age)),
                                )
                                .when_some(line.warning, |view, warning| {
                                    view.child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().warning_foreground)
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

    fn component_filter_button(
        &self,
        id: &'static str,
        filter: ComponentFilterPreference,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .label(label)
            .selected(self.state.settings.component_filter == filter)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_component_filter(filter, cx);
            }))
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

    fn detail_tab_button(
        &self,
        id: &'static str,
        tab: DetailTab,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .label(label)
            .selected(self.detail_tab == tab)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.detail_tab = tab;
                cx.notify();
            }))
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
        let disable_id = component.core_id.clone();
        let verify_id = component.core_id.clone();
        let restore_id = component.core_id.clone();
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
                    .child(self.key_value_row(
                        c.restart_requirement,
                        self.restart_requirement_label(component.restart_requirement),
                        cx,
                    ))
                    .child(self.key_value_row(c.current_version, component.version_label(), cx))
                    .child(self.key_value_row(
                        c.health,
                        self.status_tag(component.state, pending),
                        cx,
                    )),
                cx,
            ),
            DetailTab::Versions => self.surface(
                v_flex()
                    .gap_2()
                    .child(
                        self.key_value_row(
                            c.current_version,
                            component
                                .installed_version
                                .clone()
                                .unwrap_or_else(|| c.none.to_string()),
                            cx,
                        ),
                    )
                    .child(self.key_value_row(
                        c.available_version,
                        component.available_version.clone(),
                        cx,
                    ))
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
                                    .child(self.kind_tag(component.kind))
                                    .child(self.status_tag(component.state, pending)),
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
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(self.detail_tab_button(
                        "detail-overview",
                        DetailTab::Overview,
                        c.overview_tab,
                        cx,
                    ))
                    .child(self.detail_tab_button(
                        "detail-versions",
                        DetailTab::Versions,
                        c.versions_tab,
                        cx,
                    ))
                    .child(self.detail_tab_button(
                        "detail-paths",
                        DetailTab::Permissions,
                        c.files_touched,
                        cx,
                    )),
            )
            .child(body)
            .child(
                h_flex()
                    .gap_3()
                    .flex_wrap()
                    .when(
                        matches!(
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
                    ),
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
            .child(self.page_heading(c.updates_title, c.components_subtitle, false, compact))
            .when(updates.is_empty(), |view| {
                view.child(
                    self.empty_state(
                        IconName::Check,
                        c.no_updates_title,
                        c.no_updates_subtitle,
                        Button::new("updates-components")
                            .label(c.components)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate(Page::Components, cx);
                            })),
                        cx,
                    ),
                )
            })
            .when(!updates.is_empty(), |view| {
                view.child(
                    self.surface(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_4()
                            .items_center()
                            .justify_between()
                            .flex_wrap()
                            .child(
                                v_flex()
                                    .min_w(px(220.0))
                                    .flex_1()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_2xl()
                                            .font_bold()
                                            .child(updates.len().to_string()),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(c.ready_to_review),
                                    ),
                            )
                            .child(
                                Button::new("updates-review")
                                    .primary()
                                    .label(c.review_changes)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.prepare_update_all(cx);
                                    })),
                            ),
                        cx,
                    ),
                )
                .children(updates.iter().map(|component| {
                    v_flex()
                        .gap_2()
                        .child(self.component_card(component, compact, cx))
                        .child(self.release_notes_surface(component, cx))
                }))
            })
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
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(self.activity_filter_button(
                                "activity-all",
                                ActivityFilter::All,
                                c.all_activity,
                                cx,
                            ))
                            .child(self.activity_filter_button(
                                "activity-success",
                                ActivityFilter::Success,
                                c.successful,
                                cx,
                            ))
                            .child(self.activity_filter_button(
                                "activity-warning",
                                ActivityFilter::Warning,
                                c.warnings,
                                cx,
                            ))
                            .child(self.activity_filter_button(
                                "activity-failure",
                                ActivityFilter::Failure,
                                c.failures,
                                cx,
                            )),
                    )
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

    fn activity_filter_button(
        &self,
        id: &'static str,
        filter: ActivityFilter,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .label(label)
            .selected(self.activity_filter == filter)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.activity_filter = filter;
                cx.notify();
            }))
    }

    fn activity_row(
        &self,
        entry: &manager_core::ActivityRecord,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let tag = match entry.kind {
            ActivityKind::Success | ActivityKind::RecoverySuccess => {
                Tag::success().small().rounded_full().child(c.successful)
            }
            ActivityKind::Failure => Tag::danger().small().rounded_full().child(c.failed),
            ActivityKind::Warning
            | ActivityKind::RecoveryPartial
            | ActivityKind::ManualRecovery => {
                Tag::warning().small().rounded_full().child(c.warnings)
            }
            ActivityKind::Information => Tag::secondary().small().rounded_full().child(c.activity),
        };
        let operation = entry
            .operation
            .map(|operation| self.operation_label(operation))
            .unwrap_or(c.none);
        let detail = entry
            .stage
            .map(|stage| self.stage_label(stage))
            .unwrap_or_else(|| self.evidence_label(entry.evidence.as_deref()));
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

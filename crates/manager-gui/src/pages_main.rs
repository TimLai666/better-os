use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonGroup, ButtonVariants},
    scroll::ScrollableElement,
    tag::Tag,
    *,
};

use crate::{
    app::ManagerApp,
    i18n::copy,
    model::{ActivityFilter, ActivityKind, COMPONENTS, ComponentState, DetailTab, Page},
};

impl ManagerApp {
    pub(crate) fn first_run_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let steps = v_flex()
            .min_w(px(320.0))
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
            ))
            .child(self.bullet_row(
                IconName::WindowRestore,
                c.create_restore_point,
                c.create_restore_point_description,
                cx,
            ));

        let compatibility = self.surface(
            v_flex()
                .gap_3()
                .child(div().text_lg().font_semibold().child(c.compatibility_check))
                .child(self.key_value_row(c.distribution, c.distribution_value, cx))
                .child(self.key_value_row(c.desktop_session, c.desktop_session_value, cx))
                .child(self.key_value_row(c.architecture, "x86_64", cx))
                .child(self.key_value_row(c.package_tool, c.ready, cx))
                .child(self.key_value_row(c.disk_space, c.free_space, cx)),
            cx,
        );

        v_flex()
            .size_full()
            .min_w_0()
            .bg(rgb(0xf4f7fb))
            .child(
                h_flex()
                    .min_h(px(72.0))
                    .px_6()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(rgb(0xd9e1ec))
                    .bg(rgb(0xffffff))
                    .child(
                        div()
                            .size_9()
                            .rounded(px(8.0))
                            .bg(rgb(0x4f6df5))
                            .text_color(rgb(0xffffff))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(Icon::new(IconName::GalleryVerticalEnd)),
                    )
                    .child(
                        v_flex()
                            .child(div().font_semibold().child(c.brand_name))
                            .child(div().text_xs().text_color(rgb(0x667085)).child(c.manager)),
                    ),
            )
            .child(
                div().flex_1().min_h_0().overflow_y_scrollbar().p_5().child(
                    div().w_full().flex().justify_center().child(
                        v_flex()
                            .w_full()
                            .max_w(px(1180.0))
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
                                        .child(
                                            v_flex()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .text_2xl()
                                                        .font_bold()
                                                        .child(c.welcome_title),
                                                )
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(c.welcome_description),
                                                ),
                                        )
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .min_w_0()
                                                .gap_5()
                                                .items_start()
                                                .flex_wrap()
                                                .child(steps)
                                                .child(
                                                    div()
                                                        .min_w(px(300.0))
                                                        .flex_1()
                                                        .child(compatibility),
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
                                                            this.navigate(Page::Overview, cx);
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
        let installed_count = self.installed_count().to_string();
        let update_count = self.update_plan_count().to_string();
        let recent_install = format!("{} 0.1.0", c.touchpad_name);
        v_flex()
            .gap_6()
            .child(self.page_heading(c.system_healthy, c.overview_subtitle, false, compact))
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .items_stretch()
                    .flex_wrap()
                    .children([
                        self.metric_card(
                            c.installed,
                            installed_count,
                            c.all_active,
                            IconName::Inbox,
                            cx,
                        ),
                        self.metric_card(
                            c.updates_ready,
                            update_count,
                            c.ready_to_review,
                            IconName::ArrowDown,
                            cx,
                        ),
                        self.metric_card(
                            c.health,
                            c.healthy,
                            c.failed_checks,
                            IconName::CircleCheck,
                            cx,
                        ),
                        div()
                            .min_w(px(220.0))
                            .flex_1()
                            .child(
                                self.surface(
                                    v_flex()
                                        .gap_3()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_semibold()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(c.compatibility),
                                        )
                                        .child(
                                            Tag::success()
                                                .small()
                                                .rounded_full()
                                                .child(c.supported),
                                        )
                                        .child(div().text_sm().child(c.platform_summary)),
                                    cx,
                                ),
                            )
                            .into_any_element(),
                    ]),
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
                        COMPONENTS[..4]
                            .iter()
                            .copied()
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
                    .child(
                        self.surface(
                            v_flex()
                                .gap_2()
                                .child(self.activity_row(
                                    ActivityKind::Success,
                                    c.activity_touchpad_installed,
                                    "15:24",
                                    recent_install,
                                    cx,
                                ))
                                .child(self.activity_row(
                                    ActivityKind::Information,
                                    c.activity_catalog_refreshed,
                                    "15:18",
                                    "6 components",
                                    cx,
                                ))
                                .child(self.activity_row(
                                    ActivityKind::Success,
                                    c.activity_health_completed,
                                    "15:17",
                                    c.all_checks_passed,
                                    cx,
                                )),
                            cx,
                        ),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn components_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let query = self.search_query.trim().to_ascii_lowercase();
        let visible: Vec<_> = COMPONENTS
            .iter()
            .copied()
            .filter(|component| {
                query.is_empty()
                    || self
                        .component_name(component.id)
                        .to_ascii_lowercase()
                        .contains(&query)
                    || self
                        .purpose(component.id)
                        .to_ascii_lowercase()
                        .contains(&query)
            })
            .collect();

        v_flex()
            .gap_5()
            .child(self.page_heading(c.components_title, c.components_subtitle, true, compact))
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
                        .into_iter()
                        .map(|component| self.component_card(component, compact, cx)),
                )
            })
            .into_any_element()
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

    fn component_detail_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let component = self.selected_component();
        match self.detail_tab {
            DetailTab::Overview => h_flex()
                .w_full()
                .min_w_0()
                .gap_4()
                .items_stretch()
                .flex_wrap()
                .child(
                    div().min_w(px(340.0)).flex_1().child(
                        self.surface(
                            v_flex()
                                .gap_4()
                                .child(div().text_lg().font_semibold().child(c.what_it_does))
                                .child(self.detail(component.id))
                                .child(div().font_semibold().child(c.system_integration))
                                .child(self.bullet_row(
                                    IconName::Check,
                                    c.service_running,
                                    c.passed_detail,
                                    cx,
                                ))
                                .child(self.bullet_row(
                                    IconName::Check,
                                    c.configuration_valid,
                                    c.passed_detail,
                                    cx,
                                ))
                                .child(self.bullet_row(
                                    IconName::Check,
                                    c.no_conflicts,
                                    c.passed_detail,
                                    cx,
                                )),
                            cx,
                        ),
                    ),
                )
                .child(
                    div().min_w(px(300.0)).flex_1().child(
                        self.surface(
                            v_flex()
                                .gap_4()
                                .child(
                                    div()
                                        .text_lg()
                                        .font_semibold()
                                        .child(c.performance_evidence),
                                )
                                .child(self.key_value_row(c.idle_cpu, "0.2%", cx))
                                .child(self.key_value_row(c.idle_memory, "42 MB", cx))
                                .child(self.key_value_row(c.last_benchmark, c.passed, cx))
                                .child(
                                    Button::new("view-methodology")
                                        .link()
                                        .label(c.view_methodology),
                                ),
                            cx,
                        ),
                    ),
                )
                .into_any_element(),
            DetailTab::Versions => self.surface(
                v_flex()
                    .gap_4()
                    .child(self.key_value_row(
                        c.current_version,
                        component.installed_version.unwrap_or("—"),
                        cx,
                    ))
                    .child(self.key_value_row(
                        c.available_version,
                        component.available_version.unwrap_or("—"),
                        cx,
                    ))
                    .child(self.key_value_row(c.download_size, component.download_size, cx))
                    .child(self.key_value_row(c.restore_available, c.supported, cx))
                    .child(div().font_semibold().child(c.release_notes))
                    .child(self.detail(component.id)),
                cx,
            ),
            DetailTab::Permissions => self.surface(
                v_flex()
                    .gap_4()
                    .child(div().text_lg().font_semibold().child(c.permissions_tab))
                    .child(c.no_privileged_actions)
                    .child(self.bullet_row(
                        IconName::CircleCheck,
                        c.component_health_checks,
                        c.health_passed_detail,
                        cx,
                    ))
                    .child(self.bullet_row(
                        IconName::Settings,
                        c.system_integration,
                        c.files_touched,
                        cx,
                    )),
                cx,
            ),
            DetailTab::Benchmarks => self.surface(
                h_flex()
                    .w_full()
                    .gap_4()
                    .items_stretch()
                    .flex_wrap()
                    .child(self.metric_card(c.idle_cpu, "0.2%", c.passed, IconName::Inspector, cx))
                    .child(self.metric_card(c.idle_memory, "42 MB", c.passed, IconName::Inbox, cx))
                    .child(self.metric_card(
                        c.last_benchmark,
                        c.passed,
                        c.view_methodology,
                        IconName::Check,
                        cx,
                    )),
                cx,
            ),
        }
    }

    pub(crate) fn component_detail_page(
        &self,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let component = self.selected_component();
        let id = component.id;

        let primary_action = match component.state {
            ComponentState::UpdateAvailable => Some(
                Button::new("detail-update")
                    .primary()
                    .label(c.update)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.prepare_component_change(id, cx);
                    })),
            ),
            ComponentState::Available => Some(
                Button::new("detail-install")
                    .primary()
                    .label(c.install)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.prepare_component_change(id, cx);
                    })),
            ),
            _ => None,
        };

        v_flex()
            .gap_5()
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .child(
                        Button::new("back-components")
                            .label(c.back)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate(Page::Components, cx);
                            })),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .flex_wrap()
                            .child(Button::new("disable-component").label(c.disable).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.open_disable_dialog(id, cx);
                                }),
                            ))
                            .when_some(primary_action, |row, action| row.child(action)),
                    ),
            )
            .child(
                self.surface(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_4()
                        .items_center()
                        .justify_between()
                        .flex_wrap()
                        .child(
                            h_flex()
                                .min_w(px(280.0))
                                .flex_1()
                                .min_w_0()
                                .gap_4()
                                .items_center()
                                .child(
                                    div()
                                        .size_12()
                                        .flex_shrink_0()
                                        .rounded(cx.theme().radius)
                                        .bg(cx.theme().secondary)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(Icon::new(self.component_icon(component.id))),
                                )
                                .child(
                                    v_flex()
                                        .min_w_0()
                                        .gap_2()
                                        .child(
                                            div()
                                                .text_2xl()
                                                .font_bold()
                                                .child(self.component_name(component.id)),
                                        )
                                        .child(self.purpose(component.id))
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .flex_wrap()
                                                .child(self.kind_tag(component.kind))
                                                .child(self.status_tag(component.state))
                                                .child(self.restart_tag(component.restart)),
                                        ),
                                ),
                        )
                        .child(
                            v_flex()
                                .gap_2()
                                .child(self.key_value_row(
                                    c.current_version,
                                    component.installed_version.unwrap_or("—"),
                                    cx,
                                ))
                                .child(self.key_value_row(
                                    c.available_version,
                                    component.available_version.unwrap_or("—"),
                                    cx,
                                )),
                        ),
                    cx,
                ),
            )
            .child(
                ButtonGroup::new("component-detail-tabs")
                    .child(self.detail_tab_button(
                        "tab-overview",
                        DetailTab::Overview,
                        c.overview_tab,
                        cx,
                    ))
                    .child(self.detail_tab_button(
                        "tab-versions",
                        DetailTab::Versions,
                        c.versions_tab,
                        cx,
                    ))
                    .child(self.detail_tab_button(
                        "tab-permissions",
                        DetailTab::Permissions,
                        c.permissions_tab,
                        cx,
                    ))
                    .child(self.detail_tab_button(
                        "tab-benchmarks",
                        DetailTab::Benchmarks,
                        c.benchmarks_tab,
                        cx,
                    )),
            )
            .child(self.component_detail_tab(cx))
            .when(compact, |view| view.pb_4())
            .into_any_element()
    }

    pub(crate) fn updates_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        if self.show_no_updates {
            return v_flex()
                .gap_5()
                .child(self.page_heading(c.updates_title, c.updates_subtitle, false, compact))
                .child(
                    self.empty_state(
                        IconName::Check,
                        c.no_updates_title,
                        c.no_updates_subtitle,
                        Button::new("check-updates-again")
                            .primary()
                            .label(c.check_again)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_no_updates = false;
                                cx.notify();
                            })),
                        cx,
                    ),
                )
                .into_any_element();
        }

        let release_cards = [
            (
                format!("{} 0.1.1", self.component_name("manager")),
                c.manager_release_detail,
                "4.8 MB",
            ),
            (
                format!("{} 0.1.1", self.component_name("touchpad")),
                c.touchpad_release_detail,
                "2.1 MB",
            ),
            (
                format!("{} 0.2.0", self.component_name("monitor")),
                c.monitor_release_detail,
                "18.4 MB",
            ),
        ];
        let update_count = self.update_plan_count().to_string();

        v_flex()
            .gap_5()
            .child(self.page_heading(c.updates_title, c.updates_subtitle, false, compact))
            .child(
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
                                .min_w(px(280.0))
                                .flex_1()
                                .gap_2()
                                .child(div().text_xl().font_semibold().child(update_count))
                                .child(c.updates_summary),
                        )
                        .child(
                            Button::new("review-updates")
                                .primary()
                                .label(c.review_changes)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.prepare_update_all(cx);
                                })),
                        ),
                    cx,
                ),
            )
            .children(
                COMPONENTS
                    .iter()
                    .copied()
                    .filter(|component| component.state == ComponentState::UpdateAvailable)
                    .map(|component| self.component_card(component, compact, cx)),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .items_stretch()
                    .flex_wrap()
                    .children([
                        self.metric_card(c.total_download, "25.3 MB", "3", IconName::ArrowDown, cx),
                        self.metric_card(
                            c.restore_points,
                            c.available_for_all,
                            c.supported,
                            IconName::WindowRestore,
                            cx,
                        ),
                        self.metric_card(
                            c.compatibility,
                            c.compatibility_result,
                            c.supported,
                            IconName::CircleCheck,
                            cx,
                        ),
                    ]),
            )
            .child(self.section_header(c.release_notes, None, None, cx))
            .children(release_cards.into_iter().map(|(title, detail, size)| {
                self.surface(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_3()
                        .items_start()
                        .justify_between()
                        .flex_wrap()
                        .child(
                            v_flex()
                                .min_w(px(280.0))
                                .flex_1()
                                .gap_2()
                                .child(div().font_semibold().child(title))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(detail),
                                ),
                        )
                        .child(Tag::secondary().small().rounded_full().child(size)),
                    cx,
                )
            }))
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

    pub(crate) fn activity_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let filter = self.activity_filter;
        let show_success = matches!(filter, ActivityFilter::All | ActivityFilter::Success);
        let show_warning = matches!(filter, ActivityFilter::All | ActivityFilter::Warning);
        let show_failure = matches!(filter, ActivityFilter::All | ActivityFilter::Failure);

        v_flex()
            .gap_5()
            .child(self.page_heading(c.activity_title, c.activity_subtitle, false, compact))
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .child(
                        ButtonGroup::new("activity-filter")
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
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(Button::new("export-activity").label(c.export_log))
                            .child(
                                Button::new("clear-activity")
                                    .danger()
                                    .outline()
                                    .label(c.clear_history)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.activity_cleared = true;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .when(self.activity_cleared, |view| {
                view.child(
                    self.empty_state(
                        IconName::WindowRestore,
                        c.no_activity_title,
                        c.no_activity_subtitle,
                        Button::new("restore-demo-activity")
                            .label(c.retry)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.activity_cleared = false;
                                cx.notify();
                            })),
                        cx,
                    ),
                )
            })
            .when(!self.activity_cleared, |view| {
                view.child(
                    self.surface(
                        v_flex()
                            .gap_1()
                            .when(show_success, |list| {
                                list.child(self.activity_row(
                                    ActivityKind::Success,
                                    c.activity_update_installed,
                                    "15:24",
                                    "0.1.0 → 0.1.1",
                                    cx,
                                ))
                            })
                            .when(show_warning, |list| {
                                list.child(self.activity_row(
                                    ActivityKind::Warning,
                                    c.activity_check_warning,
                                    "14:46",
                                    c.warning_detail,
                                    cx,
                                ))
                            })
                            .when(show_failure, |list| {
                                list.child(self.activity_row(
                                    ActivityKind::Failure,
                                    c.activity_restore_completed,
                                    c.yesterday,
                                    c.failed_service_detail,
                                    cx,
                                ))
                            })
                            .when(show_success, |list| {
                                list.child(self.activity_row(
                                    ActivityKind::Information,
                                    c.activity_catalog_updated,
                                    c.yesterday,
                                    "6 components",
                                    cx,
                                ))
                            }),
                        cx,
                    ),
                )
            })
            .into_any_element()
    }
}

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu as _, PopupMenuItem},
    tag::Tag,
    *,
};
use manager_core::{
    ComponentStatus, DesiredOperation, DiskSpaceCheck, HealthState, RestartRequirement,
};

use crate::{
    app::ManagerApp,
    i18n::copy,
    model::{ComponentInfo, ComponentKind, Page},
};

impl ManagerApp {
    pub(crate) fn surface(&self, child: impl IntoElement, cx: &mut Context<Self>) -> AnyElement {
        better_ui::surface(
            child,
            cx.theme().border,
            cx.theme().background,
            cx.theme().radius,
        )
        .into_any_element()
    }

    pub(crate) fn metric_card(
        &self,
        label: &'static str,
        value: String,
        detail: &'static str,
        icon: IconName,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .min_w(px(210.0))
            .flex_1()
            .child(
                self.surface(
                    v_flex()
                        .min_w_0()
                        .gap_3()
                        .child(
                            h_flex()
                                .min_w_0()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .min_w_0()
                                        .text_sm()
                                        .font_semibold()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(label),
                                )
                                .child(Icon::new(icon).small().text_color(cx.theme().primary)),
                        )
                        .child(div().text_2xl().font_bold().child(value))
                        .child(
                            div()
                                .min_w_0()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(detail),
                        ),
                    cx,
                ),
            )
            .into_any_element()
    }

    pub(crate) fn section_header(
        &self,
        title: &'static str,
        subtitle: Option<&'static str>,
        action: Option<Button>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .items_end()
            .justify_between()
            .flex_wrap()
            .child(
                v_flex()
                    .min_w(px(220.0))
                    .flex_1()
                    .gap_1()
                    .child(div().text_lg().font_semibold().child(title))
                    .when_some(subtitle, |view, subtitle| {
                        view.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(subtitle),
                        )
                    }),
            )
            .when_some(action, |row, action| row.child(action))
            .into_any_element()
    }

    pub(crate) fn key_value_row(
        &self,
        label: impl IntoElement,
        value: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .items_start()
            .justify_between()
            .flex_wrap()
            .py_2()
            .child(
                div()
                    .min_w(px(180.0))
                    .flex_1()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(div().min_w_0().text_sm().font_medium().child(value))
            .into_any_element()
    }

    pub(crate) fn bullet_row(
        &self,
        icon: IconName,
        title: &'static str,
        detail: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .items_start()
            .child(
                div()
                    .size_7()
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(cx.theme().secondary)
                    .text_color(cx.theme().primary)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(icon).small()),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .gap_1()
                    .child(div().text_sm().font_semibold().child(title))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(detail),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn empty_state(
        &self,
        icon: IconName,
        title: &'static str,
        detail: &'static str,
        action: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.surface(
            v_flex()
                .w_full()
                .items_center()
                .text_center()
                .gap_3()
                .py_8()
                .child(
                    div()
                        .size_11()
                        .rounded_full()
                        .bg(cx.theme().secondary)
                        .text_color(cx.theme().primary)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(icon).large()),
                )
                .child(div().text_lg().font_semibold().child(title))
                .child(
                    div()
                        .max_w(px(560.0))
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(detail),
                )
                .child(action),
            cx,
        )
    }

    pub(crate) fn status_tag(&self, status: ComponentStatus, pending: bool) -> Tag {
        let c = copy(self.locale);
        if pending {
            return Tag::info().small().rounded_full().child(c.ready_to_install);
        }
        match status {
            ComponentStatus::Available => Tag::info().small().rounded_full().child(c.available),
            ComponentStatus::Downloading => Tag::info().small().rounded_full().child(c.downloading),
            ComponentStatus::ReadyToInstall => {
                Tag::info().small().rounded_full().child(c.ready_to_install)
            }
            ComponentStatus::Installing => {
                Tag::info().small().rounded_full().child(c.installing_files)
            }
            ComponentStatus::Verifying => {
                Tag::info().small().rounded_full().child(c.checking_works)
            }
            ComponentStatus::Healthy => Tag::success().small().rounded_full().child(c.healthy),
            ComponentStatus::UpdateAvailable => Tag::warning()
                .small()
                .rounded_full()
                .child(c.update_available),
            ComponentStatus::Disabled => Tag::secondary().small().rounded_full().child(c.disabled),
            ComponentStatus::Incompatible => Tag::danger()
                .outline()
                .small()
                .rounded_full()
                .child(c.incompatible),
            ComponentStatus::Degraded => Tag::warning().small().rounded_full().child(c.degraded),
            ComponentStatus::Failed => Tag::danger().small().rounded_full().child(c.failed),
            ComponentStatus::RestoreAvailable => Tag::danger()
                .small()
                .rounded_full()
                .child(c.restore_available_status),
        }
    }

    pub(crate) fn kind_tag(&self, kind: ComponentKind) -> Tag {
        let c = copy(self.locale);
        match kind {
            ComponentKind::Replacement => Tag::info()
                .outline()
                .small()
                .rounded_full()
                .child(c.replacement),
            ComponentKind::Enhancement => Tag::success()
                .outline()
                .small()
                .rounded_full()
                .child(c.enhancement),
            ComponentKind::Diagnostic => Tag::secondary()
                .outline()
                .small()
                .rounded_full()
                .child(c.diagnostic),
        }
    }

    fn health_tag(&self, health: HealthState) -> Tag {
        let c = copy(self.locale);
        match health {
            HealthState::Healthy => Tag::success()
                .outline()
                .small()
                .rounded_full()
                .child(c.healthy),
            HealthState::Degraded => Tag::warning()
                .outline()
                .small()
                .rounded_full()
                .child(c.degraded),
            HealthState::Failed => Tag::danger()
                .outline()
                .small()
                .rounded_full()
                .child(c.failed),
        }
    }

    pub(crate) fn component_icon(&self, id: &str) -> IconName {
        match id {
            "manager" => IconName::Settings,
            "monitor" => IconName::Inspector,
            "files" => IconName::Folder,
            _ => IconName::Inbox,
        }
    }

    pub(crate) fn component_action_button(
        &self,
        component: &ComponentInfo,
        cx: &mut Context<Self>,
    ) -> Button {
        let c = copy(self.locale);
        let id = component.ui_id;
        if self.is_pending(&component.core_id) {
            return Button::new(format!("review-{id}"))
                .label(c.review_changes)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.navigate(Page::ReviewChanges, cx);
                }));
        }
        match component.state {
            ComponentStatus::Available => Button::new(format!("install-{id}"))
                .primary()
                .label(c.install)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.prepare_component_change(id, cx);
                })),
            ComponentStatus::UpdateAvailable => Button::new(format!("update-{id}"))
                .primary()
                .label(c.update)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.prepare_component_change(id, cx);
                })),
            ComponentStatus::Disabled => Button::new(format!("enable-{id}"))
                .primary()
                .label(c.enable)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.prepare_component_change(id, cx);
                })),
            ComponentStatus::RestoreAvailable
            | ComponentStatus::Failed
            | ComponentStatus::Degraded => Button::new(format!("recover-{id}"))
                .danger()
                .label(c.view_recovery)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_component(id, cx);
                })),
            _ => Button::new(format!("details-{id}"))
                .label(c.details)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_component(id, cx);
                })),
        }
    }

    fn component_overflow_menu(
        &self,
        component: &ComponentInfo,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let id = component.ui_id;
        let can_change = !self.is_pending(&component.core_id)
            && component.state != ComponentStatus::Incompatible;
        let installed = component.installed_version.is_some();
        let enabled = component.enabled;
        let restore_available = component.restore_available;
        let view = cx.entity();
        let details_view = view.clone();
        let enable_view = view.clone();
        let disable_view = view.clone();
        let verify_view = view.clone();
        let restore_view = view.clone();
        let remove_view = view;

        Button::new(format!("component-actions-{id}"))
            .ghost()
            .icon(IconName::Ellipsis)
            .tooltip(c.more_actions)
            .dropdown_menu(move |menu, window, _| {
                let menu = menu.item(PopupMenuItem::new(c.details).on_click(
                    window.listener_for(&details_view, move |this, _, _, cx| {
                        this.open_component(id, cx)
                    }),
                ));
                let menu = if can_change && installed && enabled {
                    menu.item(PopupMenuItem::new(c.disable).on_click(window.listener_for(
                        &disable_view,
                        move |this, _, _, cx| {
                            this.prepare_component_operation(id, DesiredOperation::Disable, cx)
                        },
                    )))
                } else if can_change && installed {
                    menu.item(PopupMenuItem::new(c.enable).on_click(window.listener_for(
                        &enable_view,
                        move |this, _, _, cx| {
                            this.prepare_component_operation(id, DesiredOperation::Enable, cx)
                        },
                    )))
                } else {
                    menu
                };
                let menu = if can_change && installed {
                    menu.item(
                        PopupMenuItem::new(c.retry_check).on_click(window.listener_for(
                            &verify_view,
                            move |this, _, _, cx| {
                                this.prepare_component_operation(id, DesiredOperation::Verify, cx)
                            },
                        )),
                    )
                } else {
                    menu
                };
                let menu =
                    if can_change && restore_available {
                        menu.item(PopupMenuItem::new(c.restore_previous).on_click(
                            window.listener_for(&restore_view, move |this, _, _, cx| {
                                this.prepare_component_operation(id, DesiredOperation::Restore, cx)
                            }),
                        ))
                    } else {
                        menu
                    };
                if can_change && installed {
                    menu.separator().item(PopupMenuItem::new(c.remove).on_click(
                        window.listener_for(&remove_view, move |this, _, _, cx| {
                            this.prepare_component_operation(id, DesiredOperation::Remove, cx)
                        }),
                    ))
                } else {
                    menu
                }
            })
            .into_any_element()
    }

    pub(crate) fn component_card(
        &self,
        component: &ComponentInfo,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = component.ui_id;
        let pending = self.is_pending(&component.core_id);
        let action = self.component_action_button(component, cx);
        let overflow = self.component_overflow_menu(component, cx);
        let c = copy(self.locale);
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
                        .min_w(px(if compact { 220.0 } else { 330.0 }))
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
                                .child(Icon::new(self.component_icon(id)).small()),
                        )
                        .child(
                            v_flex()
                                .min_w_0()
                                .gap_1()
                                .child(div().font_semibold().child(self.component_name(id)))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(self.purpose(id)),
                                ),
                        ),
                )
                .child(
                    h_flex()
                        .min_w_0()
                        .gap_2()
                        .items_center()
                        .flex_wrap()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(component.version_label()),
                        )
                        .child(self.kind_tag(component.kind))
                        .child(self.status_tag(component.state, pending)),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .flex_wrap()
                        .when(component.installed_version.is_some(), |row| {
                            row.child(self.health_tag(component.health))
                        })
                        .when(component.restore_available, |row| {
                            row.child(
                                Tag::info()
                                    .outline()
                                    .small()
                                    .rounded_full()
                                    .child(c.restore_available),
                            )
                        })
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{}: {}",
                                    c.restart_requirement,
                                    self.restart_requirement_label(component.restart_requirement)
                                )),
                        )
                        .child(action)
                        .child(overflow),
                ),
            cx,
        )
    }

    pub(crate) fn release_notes_surface(
        &self,
        component: &ComponentInfo,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        if component.release_notes.is_empty() {
            return self.surface(
                v_flex()
                    .gap_1()
                    .child(div().text_sm().font_semibold().child(c.release_notes))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(c.no_release_notes),
                    ),
                cx,
            );
        }
        self.surface(
            v_flex()
                .gap_1()
                .child(div().text_sm().font_semibold().child(c.release_notes))
                .children(component.release_notes.iter().map(|note| {
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(note.clone())
                })),
            cx,
        )
    }

    pub(crate) fn byte_count_label(&self, bytes: Option<u64>) -> String {
        bytes
            .map(format_byte_count)
            .unwrap_or_else(|| copy(self.locale).not_declared.to_string())
    }

    pub(crate) fn disk_space_label(&self, check: DiskSpaceCheck) -> String {
        let c = copy(self.locale);
        match check {
            DiskSpaceCheck::NotRequired => c.not_required.to_string(),
            DiskSpaceCheck::NotDeclared => c.not_declared.to_string(),
            DiskSpaceCheck::Sufficient {
                required_bytes,
                available_bytes,
            } => format!(
                "{} {} · {} {}",
                format_byte_count(required_bytes),
                c.required,
                format_byte_count(available_bytes),
                c.available_space
            ),
        }
    }

    pub(crate) fn restart_requirement_label(
        &self,
        requirement: RestartRequirement,
    ) -> &'static str {
        match requirement {
            RestartRequirement::NotDeclared => copy(self.locale).not_declared,
        }
    }

    pub(crate) fn error_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.planning_error.map(|_| {
            self.surface(
                h_flex()
                    .min_w_0()
                    .gap_3()
                    .items_start()
                    .child(Icon::new(IconName::Info).small().text_color(cx.theme().red))
                    .child(
                        div()
                            .min_w_0()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.error_message()),
                    ),
                cx,
            )
        })
    }
}

fn format_byte_count(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    if bytes >= GIB {
        format!("{} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{} KiB", bytes / KIB)
    } else {
        format!("{bytes} B")
    }
}

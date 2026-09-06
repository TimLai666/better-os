use better_core::ComponentIcon;
use better_ui::{BadgeStyle, StatusPill, StatusTone};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu as _, PopupMenuItem},
    *,
};
use manager_core::{
    ComponentStatus, DesiredOperation, DiskSpaceCheck, HealthState, RestartRequirement,
};

use crate::{
    app::ManagerApp,
    i18n::{Locale, copy},
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
                // `flex_1` is what makes this a column of text rather than a
                // column of single characters. Without it the wrapper's flex
                // basis stays `auto`, a nested flex column reports its
                // min-content width for that basis — one character wide in a
                // language that breaks between every character — and nothing
                // ever asks it to grow, so the row leaves the rest of its width
                // empty. `min_w` keeps the guarantee when the row really is too
                // narrow: the enclosing row wraps instead of squeezing.
                v_flex()
                    .flex_1()
                    .min_w(px(crate::layout::STEP_LABEL_MIN_WIDTH))
                    .gap_1()
                    .child(div().min_w_0().text_sm().font_semibold().child(title))
                    .child(
                        div()
                            .min_w_0()
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

    /// The colors one status tone is drawn in.
    ///
    /// Deliberately *not* the fill a button of the same tone would use. A
    /// status is tinted and outlined — the border and the text carry the tone,
    /// the background only hints at it — so a `danger` status and a `danger`
    /// action can never be told apart by shape alone. `better_ui::Affordance`
    /// states the rule this implements.
    fn status_pill_style(&self, tone: StatusTone, cx: &mut Context<Self>) -> BadgeStyle {
        let accent = match tone {
            StatusTone::Neutral => {
                return BadgeStyle {
                    foreground: cx.theme().muted_foreground,
                    background: cx.theme().secondary,
                    border: cx.theme().border,
                };
            }
            StatusTone::Info => cx.theme().info,
            StatusTone::Success => cx.theme().success,
            StatusTone::Warning => cx.theme().warning,
            StatusTone::Danger => cx.theme().danger,
        };
        BadgeStyle {
            foreground: accent,
            background: accent.opacity(0.12),
            border: accent.opacity(0.4),
        }
    }

    /// Draw one read-only status indicator.
    ///
    /// Every status in this window goes through here, which is what keeps the
    /// promise that none of them responds to a pointer.
    pub(crate) fn pill(&self, pill: StatusPill, cx: &mut Context<Self>) -> AnyElement {
        let style = self.status_pill_style(pill.tone, cx);
        pill.render(style).into_any_element()
    }

    /// How one component's state reads. An associated function taking a
    /// locale rather than a method, so a test can enumerate every state
    /// without opening a window.
    pub(crate) fn status_pill(
        locale: Locale,
        status: ComponentStatus,
        pending: bool,
    ) -> StatusPill {
        let c = copy(locale);
        if pending {
            return StatusPill::new(c.ready_to_install, StatusTone::Info);
        }
        let (label, tone) = match status {
            ComponentStatus::Available => (c.available, StatusTone::Info),
            ComponentStatus::Downloading => (c.downloading, StatusTone::Info),
            ComponentStatus::ReadyToInstall => (c.ready_to_install, StatusTone::Info),
            ComponentStatus::Installing => (c.installing_files, StatusTone::Info),
            ComponentStatus::Verifying => (c.checking_works, StatusTone::Info),
            ComponentStatus::Healthy => (c.healthy, StatusTone::Success),
            ComponentStatus::UpdateAvailable => (c.update_available, StatusTone::Warning),
            ComponentStatus::Disabled => (c.disabled, StatusTone::Neutral),
            ComponentStatus::Incompatible => (c.incompatible, StatusTone::Danger),
            ComponentStatus::Degraded => (c.degraded, StatusTone::Warning),
            ComponentStatus::Failed => (c.failed, StatusTone::Danger),
            ComponentStatus::RestoreAvailable => (c.restore_available_status, StatusTone::Danger),
        };
        StatusPill::new(label, tone)
    }

    pub(crate) fn status_tag(
        &self,
        status: ComponentStatus,
        pending: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.pill(Self::status_pill(self.locale, status, pending), cx)
    }

    pub(crate) fn kind_pill(locale: Locale, kind: ComponentKind) -> StatusPill {
        let c = copy(locale);
        let (label, tone) = match kind {
            ComponentKind::Replacement => (c.replacement, StatusTone::Info),
            ComponentKind::Enhancement => (c.enhancement, StatusTone::Success),
            ComponentKind::Diagnostic => (c.diagnostic, StatusTone::Neutral),
        };
        StatusPill::new(label, tone)
    }

    pub(crate) fn kind_tag(&self, kind: ComponentKind, cx: &mut Context<Self>) -> AnyElement {
        self.pill(Self::kind_pill(self.locale, kind), cx)
    }

    pub(crate) fn health_pill(locale: Locale, health: HealthState) -> StatusPill {
        let c = copy(locale);
        let (label, tone) = match health {
            HealthState::Healthy => (c.healthy, StatusTone::Success),
            HealthState::Degraded => (c.degraded, StatusTone::Warning),
            HealthState::Failed => (c.failed, StatusTone::Danger),
        };
        StatusPill::new(label, tone)
    }

    fn health_tag(&self, health: HealthState, cx: &mut Context<Self>) -> AnyElement {
        self.pill(Self::health_pill(self.locale, health), cx)
    }

    /// Maps the icon a manifest declares onto a shipped glyph. The manifest
    /// chooses from a closed set, so this never guesses from a component ID.
    pub(crate) fn component_icon(&self, icon: ComponentIcon) -> IconName {
        match icon {
            ComponentIcon::Manager => IconName::Settings,
            ComponentIcon::Monitor => IconName::Inspector,
            ComponentIcon::Files => IconName::Folder,
            ComponentIcon::Launcher => IconName::LayoutDashboard,
            ComponentIcon::Touchpad => IconName::Frame,
            ComponentIcon::Generic => IconName::Inbox,
        }
    }

    pub(crate) fn component_action_button(
        &self,
        component: &ComponentInfo,
        cx: &mut Context<Self>,
    ) -> Button {
        let c = copy(self.locale);
        let id = component.core_id.clone();
        if self.is_pending(&component.core_id) {
            return Button::new(component.element_id("review"))
                .label(c.review_changes)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.navigate(Page::ReviewChanges, cx);
                }));
        }
        match component.state {
            ComponentStatus::Available => Button::new(component.element_id("install"))
                .primary()
                .label(c.install)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.prepare_component_change(&id, cx);
                })),
            ComponentStatus::UpdateAvailable => Button::new(component.element_id("update"))
                .primary()
                .label(c.update)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.prepare_component_change(&id, cx);
                })),
            ComponentStatus::Disabled => Button::new(component.element_id("enable"))
                .primary()
                .label(c.enable)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.prepare_component_change(&id, cx);
                })),
            ComponentStatus::RestoreAvailable
            | ComponentStatus::Failed
            | ComponentStatus::Degraded => Button::new(component.element_id("recover"))
                .danger()
                .label(c.view_recovery)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_component(&id, cx);
                })),
            _ => Button::new(component.element_id("details"))
                .label(c.details)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_component(&id, cx);
                })),
        }
    }

    fn component_overflow_menu(
        &self,
        component: &ComponentInfo,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let id = component.core_id.clone();
        let can_change = !self.is_pending(&component.core_id)
            && component.state != ComponentStatus::Incompatible;
        let installed = component.installed_version.is_some();
        let is_manager_itself = manager_core::is_self_component(&component.core_id);
        let enabled = component.enabled;
        let restore_available = component.restore_available;
        let view = cx.entity();
        let details_view = view.clone();
        let enable_view = view.clone();
        let disable_view = view.clone();
        let verify_view = view.clone();
        let restore_view = view.clone();
        let remove_view = view;

        Button::new(component.element_id("component-actions"))
            .ghost()
            .icon(IconName::Ellipsis)
            .tooltip(c.more_actions)
            .dropdown_menu(move |menu, window, _| {
                let details_id = id.clone();
                let menu = menu.item(PopupMenuItem::new(c.details).on_click(
                    window.listener_for(&details_view, move |this, _, _, cx| {
                        this.open_component(&details_id, cx)
                    }),
                ));
                let menu = if can_change && installed && enabled {
                    let disable_id = id.clone();
                    menu.item(PopupMenuItem::new(c.disable).on_click(window.listener_for(
                        &disable_view,
                        move |this, _, _, cx| {
                            this.prepare_component_operation(
                                &disable_id,
                                DesiredOperation::Disable,
                                cx,
                            )
                        },
                    )))
                } else if can_change && installed {
                    let enable_id = id.clone();
                    menu.item(PopupMenuItem::new(c.enable).on_click(window.listener_for(
                        &enable_view,
                        move |this, _, _, cx| {
                            this.prepare_component_operation(
                                &enable_id,
                                DesiredOperation::Enable,
                                cx,
                            )
                        },
                    )))
                } else {
                    menu
                };
                let menu = if can_change && installed {
                    let verify_id = id.clone();
                    menu.item(
                        PopupMenuItem::new(c.retry_check).on_click(window.listener_for(
                            &verify_view,
                            move |this, _, _, cx| {
                                this.prepare_component_operation(
                                    &verify_id,
                                    DesiredOperation::Verify,
                                    cx,
                                )
                            },
                        )),
                    )
                } else {
                    menu
                };
                let menu =
                    if can_change && restore_available {
                        let restore_id = id.clone();
                        menu.item(PopupMenuItem::new(c.restore_previous).on_click(
                            window.listener_for(&restore_view, move |this, _, _, cx| {
                                this.prepare_component_operation(
                                    &restore_id,
                                    DesiredOperation::Restore,
                                    cx,
                                )
                            }),
                        ))
                    } else {
                        menu
                    };
                // No remove entry for the manager itself: `manager-core`
                // refuses to plan its own removal, so offering it would only
                // produce a refusal.
                if can_change && installed && !is_manager_itself {
                    let remove_id = id.clone();
                    menu.separator().item(PopupMenuItem::new(c.remove).on_click(
                        window.listener_for(&remove_view, move |this, _, _, cx| {
                            this.prepare_component_operation(
                                &remove_id,
                                DesiredOperation::Remove,
                                cx,
                            )
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
                                .child(Icon::new(self.component_icon(component.icon)).small()),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .child(div().font_semibold().child(component.name.clone()))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(self.declared_or_not(&component.summary)),
                                )
                                .when_some(self.replacement_label(component), |view, label| {
                                    view.child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(label),
                                    )
                                }),
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
                                .child(component.version_label(c.not_installed)),
                        )
                        .child(self.kind_tag(component.kind, cx))
                        .child(self.status_tag(component.state, pending, cx)),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .flex_wrap()
                        .when(component.installed_version.is_some(), |row| {
                            row.child(self.health_tag(component.health, cx))
                        })
                        .when(component.restore_available, |row| {
                            row.child(
                                self.pill(
                                    StatusPill::new(c.restore_available, StatusTone::Info),
                                    cx,
                                ),
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

    /// The one card that says what actually went wrong.
    ///
    /// Shared by the recovery screen and the component page on purpose: a
    /// recorded failure showed up on the component page as a bare red tag, and
    /// the reason — the stage, the localized evidence, and the service's own
    /// untranslated words — was three screens away. Wherever a failure is
    /// surfaced it is surfaced whole.
    pub(crate) fn failure_card(
        &self,
        title: String,
        failure: &manager_core::FailureRecord,
        recovery_detail: Option<&'static str>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let (key, detail) = failure.evidence_parts();
        self.surface(
            v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .flex_wrap()
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .small()
                                .text_color(cx.theme().red),
                        )
                        .child(div().text_lg().font_semibold().child(title)),
                )
                .child(self.key_value_row(c.failed_stage, self.stage_label(failure.stage), cx))
                .child(self.key_value_row(c.failure_evidence, self.evidence_label(Some(key)), cx))
                // The machine detail is what the service actually said — "plan
                // targets release 24.04 but this host is 18" beats a localized
                // sentence when the user reports a failure, so it is shown,
                // untranslated, when present.
                .when_some(detail.map(str::to_string), |view, detail| {
                    view.child(self.key_value_row(c.failure_technical_detail, detail, cx))
                })
                // Labelled "recovery", not "a previous version can be
                // restored". The label used to be the latter and the value in
                // the ordinary case was the same sentence again, so the row
                // read as one string printed twice and said nothing about what
                // the recovery actually did.
                .when_some(recovery_detail, |view, recovery| {
                    view.child(self.key_value_row(c.recovery_status, recovery, cx))
                }),
            cx,
        )
    }

    /// The drift card: what disagrees, and the one way out of it.
    ///
    /// This is an Action card in the sense ticket 48 fixed: the heading and the
    /// sentences are status with no hover and no handler, and the single filled
    /// control is the only thing on it that does something. It asks before it
    /// acts, because adopting throws away the recorded restore point.
    pub(crate) fn drift_card(
        &self,
        component: &ComponentInfo,
        notice: &crate::model::DriftNotice,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let confirming = self.adopt_confirm.as_ref() == Some(&component.core_id);
        let adopt_id = component.core_id.clone();
        let ask_id = component.core_id.clone();
        self.surface(
            v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .flex_wrap()
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .small()
                                .text_color(cx.theme().yellow),
                        )
                        .child(div().text_lg().font_semibold().child(notice.title)),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(notice.detail.clone()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(notice.consequence),
                )
                .when(confirming, |view| {
                    view.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(c.adopt_host_state_confirm),
                    )
                })
                .child(if confirming {
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            Button::new("drift-adopt-confirm")
                                .primary()
                                .label(c.adopt_host_state_confirm_action)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.adopt_host_state(&adopt_id, cx);
                                })),
                        )
                        .child(Button::new("drift-adopt-cancel").label(c.cancel).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.ask_to_adopt_host_state(None, cx);
                            }),
                        ))
                        .into_any_element()
                } else {
                    Button::new("drift-adopt")
                        .primary()
                        .label(c.adopt_host_state)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.ask_to_adopt_host_state(Some(ask_id.clone()), cx);
                        }))
                        .into_any_element()
                }),
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
        let c = copy(self.locale);
        match requirement {
            RestartRequirement::NotDeclared => c.not_declared,
            RestartRequirement::NotRequired => c.restart_not_required,
            RestartRequirement::RestartApplication => c.restart_application,
            RestartRequirement::LogOut => c.restart_log_out,
            RestartRequirement::Reboot => c.restart_reboot,
        }
    }

    /// What a component takes over from or augments, as one line. Returns
    /// `None` when the manifest declares neither.
    pub(crate) fn replacement_label(&self, component: &ComponentInfo) -> Option<String> {
        let c = copy(self.locale);
        self.declared_relationship(c.replaces_label, &component.replaces)
            .or_else(|| self.declared_relationship(c.enhances_label, &component.enhances))
    }

    pub(crate) fn declared_relationship(
        &self,
        label: &'static str,
        values: &[String],
    ) -> Option<String> {
        if values.is_empty() {
            return None;
        }
        Some(format!("{label}: {}", values.join(", ")))
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

//! Drawing. Every decision this file renders was made in [`crate::model`].
//!
//! Two rules hold in here:
//!
//! - A row whose backend cannot read, apply, and verify a setting draws the
//!   explanation, never a switch. There is one place that branches on it, so
//!   the rule cannot be half-applied.
//! - Both a requested and an effective value are shown for every control, side
//!   by side, because they can differ and the difference is the point.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    scroll::ScrollableElement,
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu,
        SidebarMenuItem,
    },
    slider::Slider,
    switch::Switch,
    *,
};
use touchpad_core::{
    AccelerationProfile, ClickMethod, RestoreScope, RunState, Section, SettingId, SettingValue,
};

use crate::COMPACT_VIEWPORT_WIDTH;
use crate::app::TouchpadApp;
use crate::i18n::copy;
use crate::model::{Control, Page, SettingRow};

fn page_icon(page: Page) -> IconName {
    match page {
        Page::Overview => IconName::LayoutDashboard,
        Page::Pointer => IconName::Frame,
        Page::Scrolling => IconName::ChevronsUpDown,
        Page::Clicking => IconName::CircleCheck,
        Page::Gestures => IconName::Maximize,
        Page::Devices => IconName::HardDrive,
        Page::Diagnostics => IconName::Inspector,
    }
}

impl TouchpadApp {
    fn sidebar(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.model.locale());
        let menu = SidebarMenu::new().children(Page::ALL.map(|page| {
            SidebarMenuItem::new(page.label(c))
                .icon(page_icon(page))
                .active(self.page == page)
                .on_click(cx.listener(move |this, _, _, cx| this.navigate(page, cx)))
        }));

        Sidebar::new("touchpad-sidebar")
            .collapsible(SidebarCollapsible::Icon)
            .collapsed(compact)
            .w(px(232.0))
            .header(
                SidebarHeader::new()
                    .child(
                        div()
                            .size_8()
                            .flex_shrink_0()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().primary)
                            .text_color(cx.theme().primary_foreground)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(Icon::new(IconName::Frame)),
                    )
                    .when(!compact, |header| {
                        header.child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(div().font_semibold().child(c.brand))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(c.application),
                                ),
                        )
                    }),
            )
            .child(SidebarGroup::new(c.nav_overview).child(menu))
            .footer(
                SidebarFooter::new().child(
                    h_flex()
                        .min_w_0()
                        .items_center()
                        .gap_2()
                        .child(Icon::new(IconName::Settings).small())
                        .when(!compact, |row| {
                            row.child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_semibold()
                                            .child(self.model.session().describe()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(self.model.backend_status().name),
                                    ),
                            )
                        }),
                ),
            )
            .into_any_element()
    }

    fn top_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.model.locale());
        let pending = self.model.has_pending();
        h_flex()
            .w_full()
            .min_h(px(64.0))
            .px_5()
            .py_3()
            .gap_3()
            .items_center()
            .justify_between()
            .flex_wrap()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                div()
                    .min_w_0()
                    .text_lg()
                    .font_semibold()
                    .child(format!("{} {}", c.brand, c.application)),
            )
            .child(
                h_flex()
                    .min_w_0()
                    .gap_2()
                    .items_center()
                    .flex_wrap()
                    .child(
                        Button::new("switch-language")
                            .outline()
                            .tab_index(1)
                            .label(c.switch_language)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_locale(cx))),
                    )
                    .child(
                        Button::new("refresh")
                            .outline()
                            .tab_index(2)
                            .label(c.refresh)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    )
                    .child(
                        Button::new("discard")
                            .outline()
                            .tab_index(3)
                            .disabled(!pending)
                            .label(c.discard)
                            .on_click(cx.listener(|this, _, window, cx| this.discard(window, cx))),
                    )
                    .child(
                        Button::new("apply")
                            .primary()
                            .tab_index(4)
                            .disabled(!pending || self.busy)
                            .label(if self.busy { c.busy } else { c.apply })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.apply(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn heading(&self, title: &'static str, subtitle: &'static str) -> AnyElement {
        v_flex()
            .w_full()
            .min_w_0()
            .gap_1()
            .child(div().text_2xl().font_bold().child(title))
            .child(div().min_w_0().text_sm().opacity(0.7).child(subtitle))
            .into_any_element()
    }

    pub(crate) fn card(&self, child: impl IntoElement, cx: &Context<Self>) -> AnyElement {
        div()
            .w_full()
            .min_w_0()
            .p_4()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(child)
            .into_any_element()
    }

    fn fact(&self, label: &'static str, value: impl Into<SharedString>, cx: &Context<Self>) -> Div {
        v_flex()
            .min_w_0()
            .gap_0p5()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(
                div()
                    .min_w_0()
                    .text_sm()
                    .font_semibold()
                    .child(value.into()),
            )
    }

    /// One control. The single place that decides between a working control and
    /// an explanation.
    fn setting_row(&self, row: &SettingRow, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.model.locale());
        let setting = row.setting;
        let badges = h_flex()
            .gap_1()
            .flex_wrap()
            .when(row.pending, |this| {
                this.child(self.badge(c.pending_badge, cx.theme().warning, cx))
            })
            .when(row.drifted, |this| {
                this.child(self.badge(c.drifted_badge, cx.theme().muted, cx))
            })
            .when(row.needs_sign_out, |this| {
                this.child(self.badge(c.sign_out_badge, cx.theme().warning, cx))
            });

        let control: AnyElement = if !row.available {
            div()
                .min_w_0()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(format!(
                    "{}. {}",
                    c.unavailable,
                    row.unavailable_detail.clone().unwrap_or_default()
                ))
                .into_any_element()
        } else {
            match row.control {
                Control::Slider { .. } => self.slider_control(row, cx),
                Control::Switch => self.switch_control(row, cx),
                Control::Choice => self.choice_control(row, cx),
            }
        };

        self.card(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_3()
                        .items_center()
                        .justify_between()
                        .flex_wrap()
                        .child(div().min_w_0().text_sm().font_semibold().child(row.label))
                        .child(badges),
                )
                .child(control)
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_4()
                        .flex_wrap()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{}: {}", c.requested_value, row.requested_label))
                        .child(format!("{}: {}", c.effective_value, row.effective_label))
                        .when_some(row.previous_label.clone(), |this, previous| {
                            this.child(format!("{}: {previous}", c.previous_value))
                        }),
                )
                .when_some(row.result.clone(), |this, result| {
                    this.child(div().min_w_0().text_xs().child(result))
                })
                .id(SharedString::from(setting.key()))
                .into_any_element(),
            cx,
        )
    }

    fn badge(&self, label: &'static str, color: Hsla, cx: &Context<Self>) -> AnyElement {
        div()
            .px_2()
            .py_0p5()
            .rounded_full()
            .border_1()
            .border_color(color)
            .text_xs()
            .text_color(cx.theme().foreground)
            .child(label)
            .into_any_element()
    }

    fn slider_control(&self, row: &SettingRow, cx: &mut Context<Self>) -> AnyElement {
        let Some(slider) = self.slider(row.setting) else {
            return div().into_any_element();
        };
        h_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .items_center()
            .child(div().flex_1().min_w_0().child(Slider::new(slider)))
            .child(
                div()
                    .w(px(72.0))
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(row.requested_label.clone()),
            )
            .into_any_element()
    }

    fn switch_control(&self, row: &SettingRow, cx: &mut Context<Self>) -> AnyElement {
        let setting = row.setting;
        let checked = row.requested.as_bool().unwrap_or(false);
        Switch::new(SharedString::from(setting.key()))
            .checked(checked)
            .on_click(cx.listener(move |this, value: &bool, _window, cx| {
                this.stage_toggle(setting, *value, cx);
            }))
            .into_any_element()
    }

    /// A segmented row of buttons rather than a dropdown: every option stays
    /// visible, and each one is its own tab stop.
    fn choice_control(&self, row: &SettingRow, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.model.locale());
        let setting = row.setting;
        let options: Vec<(SettingValue, &'static str)> = match setting {
            SettingId::AccelerationProfile => AccelerationProfile::ALL
                .into_iter()
                .map(|profile| {
                    (
                        SettingValue::acceleration(profile),
                        match profile {
                            AccelerationProfile::Default => c.profile_default,
                            AccelerationProfile::Adaptive => c.profile_adaptive,
                            AccelerationProfile::Flat => c.profile_flat,
                        },
                    )
                })
                .collect(),
            _ => ClickMethod::ALL
                .into_iter()
                .map(|method| {
                    (
                        SettingValue::click(method),
                        match method {
                            ClickMethod::Default => c.method_default,
                            ClickMethod::Areas => c.method_areas,
                            ClickMethod::Fingers => c.method_fingers,
                            ClickMethod::None => c.method_none,
                        },
                    )
                })
                .collect(),
        };

        h_flex()
            .w_full()
            .min_w_0()
            .gap_2()
            .flex_wrap()
            .children(
                options
                    .into_iter()
                    .enumerate()
                    .map(|(index, (value, label))| {
                        let selected = value == row.requested;
                        let button =
                            Button::new(SharedString::from(format!("{}-{index}", setting.key())))
                                .label(label)
                                .small()
                                .tab_index(10 + index as isize)
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    this.stage_value(setting, value, cx);
                                }));
                        if selected {
                            button.primary()
                        } else {
                            button.outline()
                        }
                    }),
            )
            .into_any_element()
    }

    fn section_page(
        &self,
        title: &'static str,
        subtitle: &'static str,
        section: Section,
        extra: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.model.locale());
        let rows = self.model.rows(section);
        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(self.heading(title, subtitle))
            .children(rows.iter().map(|row| self.setting_row(row, cx)))
            .when_some(extra, |this, extra| this.child(extra))
            .child(
                h_flex().gap_2().flex_wrap().child(
                    Button::new("restore-section")
                        .outline()
                        .tab_index(30)
                        .label(c.restore_section)
                        .disabled(self.model.state().backup().is_none())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.restore_section(section, window, cx);
                        })),
                ),
            )
            .into_any_element()
    }

    fn overview_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.model.locale());
        let overview = self.model.overview();
        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(self.heading(c.overview_title, c.overview_subtitle))
            .child(
                self.card(
                    v_flex()
                        .gap_3()
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .gap_6()
                                .flex_wrap()
                                .child(self.fact(c.selected_touchpad, overview.device.clone(), cx))
                                .child(self.fact(c.session, overview.session.clone(), cx))
                                .child(self.fact(c.backend, overview.backend.clone(), cx))
                                .child(self.fact(c.health, format!("{:?}", overview.health), cx)),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .gap_6()
                                .flex_wrap()
                                .child(self.fact(
                                    c.pointer_summary,
                                    overview.pointer_summary.clone(),
                                    cx,
                                ))
                                .child(self.fact(
                                    c.scroll_summary,
                                    overview.scroll_summary.clone(),
                                    cx,
                                ))
                                .child(self.fact(
                                    c.pending_sign_out,
                                    if overview.awaiting_sign_out.is_empty() {
                                        "0".to_string()
                                    } else {
                                        overview.awaiting_sign_out.len().to_string()
                                    },
                                    cx,
                                ))
                                .child(self.fact(
                                    c.unavailable,
                                    overview.unavailable_count.to_string(),
                                    cx,
                                )),
                        ),
                    cx,
                ),
            )
            .child(self.card(
                div().text_sm().child(if overview.pending_count == 0 {
                    c.nothing_pending
                } else {
                    c.unsaved_changes
                }),
                cx,
            ))
            .into_any_element()
    }

    /// The pointer test surface: a bounded box that shows where the pointer is.
    ///
    /// It performs no system action. Moving the pointer over it is how the
    /// current sensitivity is felt before anything is applied.
    fn pointer_test(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.model.locale());
        let trace = self.pointer;
        let surface = self.surface.clone();
        self.card(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .child(div().text_sm().font_semibold().child(c.pointer_test_title))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(c.pointer_test_hint),
                )
                .child(
                    div()
                        .id("pointer-test-surface")
                        .relative()
                        .w_full()
                        .h(px(180.0))
                        .rounded(cx.theme().radius)
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().secondary)
                        .overflow_hidden()
                        .child(
                            canvas(
                                move |bounds, _window, _cx| surface.set(bounds),
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        )
                        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                            this.trace_pointer(event.position, cx);
                        }))
                        .when(!trace.inside, |this| {
                            this.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(c.pointer_test_idle),
                            )
                        })
                        .when(trace.inside, |this| {
                            this.child(
                                div()
                                    .absolute()
                                    .left(relative(trace.x))
                                    .top(relative(trace.y))
                                    .size_3()
                                    .rounded_full()
                                    .bg(cx.theme().primary),
                            )
                        }),
                ),
            cx,
        )
    }

    /// The scroll test area: real content that really scrolls, in both axes.
    fn scroll_test(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.model.locale());
        self.card(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .child(div().text_sm().font_semibold().child(c.scroll_test_title))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(c.scroll_test_hint),
                )
                .child(
                    div()
                        .id("scroll-test-area")
                        .w_full()
                        .h(px(200.0))
                        .rounded(cx.theme().radius)
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().secondary)
                        .overflow_scrollbar()
                        .child(v_flex().p_3().gap_2().children((0..40).map(|row| {
                            h_flex().gap_3().children((0..14).map(move |column| {
                                div()
                                    .flex_shrink_0()
                                    .w(px(96.0))
                                    .text_xs()
                                    .child(format!("{row:02}·{column:02}"))
                            }))
                        }))),
                ),
            cx,
        )
    }

    fn scrolling_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.model.locale());
        let linked = self.model.config().scrolling.linked_axes;
        let extra = v_flex()
            .w_full()
            .gap_4()
            .child(
                self.card(
                    h_flex().w_full().min_w_0().gap_3().items_center().child(
                        Switch::new("linked-axes")
                            .checked(linked)
                            .label(c.linked_axes)
                            .on_click(cx.listener(|this, value: &bool, window, cx| {
                                this.stage_linked_axes(*value, window, cx);
                            })),
                    ),
                    cx,
                ),
            )
            .child(self.scroll_test(cx))
            .into_any_element();
        self.section_page(
            c.scrolling_title,
            c.scrolling_subtitle,
            Section::Scrolling,
            Some(extra),
            cx,
        )
    }

    fn devices_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.model.locale());
        let devices = self.model.devices().to_vec();
        let selected = self
            .model
            .selected_device()
            .map(|device| device.identity.clone());
        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(self.heading(c.devices_title, c.devices_subtitle))
            .when(devices.is_empty(), |this| {
                this.child(self.card(div().text_sm().child(c.no_devices), cx))
            })
            .children(devices.iter().map(|device| {
                let is_selected = selected.as_deref() == Some(device.identity.as_str());
                self.card(
                    v_flex()
                        .w_full()
                        .min_w_0()
                        .gap_2()
                        .child(div().text_sm().font_semibold().child(device.describe()))
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .gap_6()
                                .flex_wrap()
                                .child(self.fact(c.device_identity, device.identity.clone(), cx))
                                .child(self.fact(
                                    c.device_capabilities,
                                    format!(
                                        "{}: {} · {}",
                                        c.contacts,
                                        device.capabilities.max_contacts,
                                        if device.capabilities.buttonpad {
                                            c.buttonpad
                                        } else {
                                            c.separate_buttons
                                        }
                                    ),
                                    cx,
                                ))
                                .child(self.fact(
                                    c.device_scope,
                                    if is_selected {
                                        c.scope_global
                                    } else {
                                        c.scope_per_device
                                    },
                                    cx,
                                ))
                                .child(self.fact(
                                    c.health,
                                    match device.state {
                                        touchpad_platform::DeviceState::Connected => {
                                            c.state_connected
                                        }
                                        touchpad_platform::DeviceState::Disconnected => {
                                            c.state_disconnected
                                        }
                                        touchpad_platform::DeviceState::Inhibited => {
                                            c.state_inhibited
                                        }
                                    },
                                    cx,
                                )),
                        ),
                    cx,
                )
            }))
            .into_any_element()
    }

    fn diagnostics_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.model.locale());
        let health = self.model.health();
        let rows = self.model.all_rows();
        let restore_rows = self.model.restore_rows(RestoreScope::All);
        let has_capture = self.model.state().backup().is_some();
        // Issue #3 wants recognized gesture events and conflict results here,
        // and no raw input data. A recognized event is a gesture identity, a
        // phase, and a percentage; the contact positions never leave the
        // recognizer.
        let gesture_lines = self.gestures.diagnostics_lines(c);

        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(self.heading(c.diagnostics_title, c.diagnostics_subtitle))
            .child(
                self.card(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().font_semibold().child(c.backend))
                        .child(
                            div()
                                .text_xs()
                                .child(self.model.backend_status().detail.clone()),
                        )
                        .children(health.checks.iter().map(|check| {
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} · {:?} · {}",
                                    check.id, check.state, check.detail
                                ))
                        })),
                    cx,
                ),
            )
            .child(
                self.card(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().font_semibold().child(c.gestures_title))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} · {}",
                                    self.gestures.adapter().describe().name,
                                    self.gestures.preset_card(c).status_label
                                )),
                        )
                        .when(gesture_lines.is_empty(), |this| {
                            this.child(div().text_xs().child(c.test_no_events))
                        })
                        .children(gesture_lines.iter().map(|line| {
                            div()
                                .min_w_0()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(line.clone())
                        })),
                    cx,
                ),
            )
            .child(
                self.card(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().font_semibold().child(c.effective_values))
                        .children(rows.iter().map(|row| {
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .gap_4()
                                .flex_wrap()
                                .text_xs()
                                .child(div().w(px(220.0)).child(row.setting.key()))
                                .child(row.requested_label.clone())
                                .child(row.effective_label.clone())
                        })),
                    cx,
                ),
            )
            .child(
                self.card(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().font_semibold().child(c.restore_actions))
                        .child(div().text_xs().child(if has_capture {
                            c.captured_before
                        } else {
                            c.nothing_captured
                        }))
                        .children(restore_rows.iter().map(|row| {
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .gap_4()
                                .flex_wrap()
                                .text_xs()
                                .child(div().w(px(220.0)).child(row.label))
                                .child(row.captured_label.clone())
                                .when_some(row.detail.clone(), |this, detail| this.child(detail))
                        }))
                        .child(
                            h_flex()
                                .gap_2()
                                .flex_wrap()
                                .child(
                                    Button::new("restore-all")
                                        .outline()
                                        .tab_index(40)
                                        .disabled(!has_capture)
                                        .label(c.restore_all)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.restore(RestoreScope::All, window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("safe-mode")
                                        .outline()
                                        .tab_index(41)
                                        .label(if self.model.safe_mode() {
                                            c.safe_mode_off
                                        } else {
                                            c.safe_mode_on
                                        })
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.toggle_safe_mode(cx)),
                                        ),
                                ),
                        ),
                    cx,
                ),
            )
            .into_any_element()
    }

    fn render_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.model.locale());
        match self.page {
            Page::Overview => self.overview_page(cx),
            Page::Pointer => {
                let extra = self.pointer_test(cx);
                self.section_page(
                    c.pointer_title,
                    c.pointer_subtitle,
                    Section::Pointer,
                    Some(extra),
                    cx,
                )
            }
            Page::Scrolling => self.scrolling_page(cx),
            Page::Clicking => self.section_page(
                c.clicking_title,
                c.clicking_subtitle,
                Section::Clicking,
                None,
                cx,
            ),
            Page::Gestures => self.gestures_page(cx),
            Page::Devices => self.devices_page(cx),
            Page::Diagnostics => self.diagnostics_page(cx),
        }
    }
}

impl Render for TouchpadApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = copy(self.model.locale());
        let compact = window.viewport_size().width < px(COMPACT_VIEWPORT_WIDTH);
        let banner = self.model.result_banner();
        let page = self.render_page(cx);

        // Mutter gives an `xdg-toplevel` client no decorations, so this window
        // draws its own or it cannot be closed, minimized, maximized or moved.
        v_flex()
            .relative()
            .size_full()
            .child(better_ui::window_chrome::title_bar(
                Icon::new(IconName::Frame).small(),
                format!("{} {}", c.brand, c.application),
                cx.theme().foreground,
            ))
            .child(
                div().relative().flex_1().min_h_0().child(
                    h_flex()
                        .size_full()
                        .min_w_0()
                        .bg(cx.theme().secondary)
                        .child(self.sidebar(compact, cx))
                        .child(
                            v_flex()
                                .h_full()
                                .flex_1()
                                .min_w_0()
                                .child(self.top_bar(cx))
                                .when(self.model.safe_mode(), |this| {
                                    this.child(
                                        div()
                                            .w_full()
                                            .px_5()
                                            .py_2()
                                            .bg(cx.theme().warning)
                                            .text_sm()
                                            .text_color(cx.theme().warning_foreground)
                                            .child(c.safe_mode_banner),
                                    )
                                })
                                .when_some(banner, |this, (state, text)| {
                                    let background = match state {
                                        RunState::Failed => cx.theme().danger,
                                        RunState::PartiallySupported
                                        | RunState::AwaitingSignOut => cx.theme().warning,
                                        _ => cx.theme().success,
                                    };
                                    this.child(
                                        div()
                                            .w_full()
                                            .px_5()
                                            .py_2()
                                            .bg(background)
                                            .text_sm()
                                            .child(text),
                                    )
                                })
                                .child(
                                    div().flex_1().min_h_0().overflow_y_scrollbar().p_5().child(
                                        div()
                                            .w_full()
                                            .flex()
                                            .justify_center()
                                            .child(div().w_full().max_w(px(1160.0)).child(page)),
                                    ),
                                ),
                        ),
                ),
            )
    }
}

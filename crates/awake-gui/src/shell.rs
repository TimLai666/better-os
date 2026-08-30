//! The frame every section is drawn inside: the sidebar, the top bar, and the
//! keyboard bindings that reach each section without a pointing device.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu,
        SidebarMenuItem,
    },
    *,
};

use crate::{app::AwakeApp, i18n::copy, model::Section};

/// One action per section, so every section has a real keybinding rather than a
/// tab order that happens to reach it.
pub(crate) mod actions {
    use gpui::actions;

    actions!(
        awake,
        [
            ShowStatus,
            ShowQuickSessions,
            ShowRules,
            ShowSessionDefaults,
            ShowBattery,
            ShowHistory,
            ShowDiagnostics,
            ShowSettings,
            Refresh,
        ]
    );
}

/// The bindings installed at startup. Kept beside `Section` so a section added
/// without a shortcut is visible as a missing line rather than as a section
/// nobody can reach from the keyboard.
pub(crate) fn key_bindings() -> Vec<KeyBinding> {
    use actions::*;
    vec![
        KeyBinding::new(Section::Status.shortcut(), ShowStatus, None),
        KeyBinding::new(Section::QuickSessions.shortcut(), ShowQuickSessions, None),
        KeyBinding::new(Section::Rules.shortcut(), ShowRules, None),
        KeyBinding::new(
            Section::SessionDefaults.shortcut(),
            ShowSessionDefaults,
            None,
        ),
        KeyBinding::new(Section::Battery.shortcut(), ShowBattery, None),
        KeyBinding::new(Section::History.shortcut(), ShowHistory, None),
        KeyBinding::new(Section::Diagnostics.shortcut(), ShowDiagnostics, None),
        KeyBinding::new(Section::Settings.shortcut(), ShowSettings, None),
        KeyBinding::new("ctrl-r", Refresh, None),
    ]
}

fn section_icon(section: Section) -> IconName {
    match section {
        Section::Status => IconName::LayoutDashboard,
        Section::QuickSessions => IconName::Play,
        Section::Rules => IconName::Bot,
        Section::SessionDefaults => IconName::Settings2,
        Section::Battery => IconName::Battery,
        Section::History => IconName::Inspector,
        Section::Diagnostics => IconName::Cpu,
        Section::Settings => IconName::Settings,
    }
}

impl AwakeApp {
    fn nav_item(&self, section: Section, cx: &mut Context<Self>) -> SidebarMenuItem {
        let c = copy(self.locale);
        SidebarMenuItem::new(section.title(c))
            .icon(section_icon(section))
            .active(self.section == section)
            .on_click(cx.listener(move |this, _, _, cx| this.navigate(section, cx)))
    }

    pub(crate) fn sidebar(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let menu =
            SidebarMenu::new().children(Section::ALL.map(|section| self.nav_item(section, cx)));

        Sidebar::new("awake-sidebar")
            .collapsible(SidebarCollapsible::Icon)
            .collapsed(compact)
            .w(px(248.0))
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
                            .child(Icon::new(IconName::Sun)),
                    )
                    .when(!compact, |header| {
                        header.child(
                            v_flex()
                                .min_w_0()
                                .child(div().font_semibold().child(c.application_name))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(c.application_tagline),
                                ),
                        )
                    }),
            )
            .child(SidebarGroup::new(c.navigation).child(menu))
            .footer(
                SidebarFooter::new().child(
                    h_flex()
                        .min_w_0()
                        .items_center()
                        .gap_2()
                        .child(Icon::new(IconName::Info).small())
                        .when(!compact, |row| {
                            row.child(
                                div()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(c.keyboard_hint),
                            )
                        }),
                ),
            )
            .into_any_element()
    }

    pub(crate) fn top_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let summary = match (&self.connection_error, &self.status) {
            (Some(_), _) => c.service_unreachable,
            (None, Some(status)) => status.summary(c),
            (None, None) => c.unknown,
        };
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
                v_flex()
                    .min_w(px(200.0))
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .min_w_0()
                            .text_lg()
                            .font_semibold()
                            .child(c.application_name),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(summary),
                    ),
            )
            .child(
                Button::new("refresh")
                    .primary()
                    .icon(IconName::Redo)
                    .label(c.refresh)
                    .loading(self.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
            )
            .into_any_element()
    }
}

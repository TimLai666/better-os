//! Sidebar, top bar, and the shared card primitives.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    input::Input,
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu,
        SidebarMenuItem,
    },
    *,
};

use crate::app::{MonitorApp, Page};
use crate::i18n::copy;

impl MonitorApp {
    fn nav_item(
        &self,
        label: &'static str,
        icon: IconName,
        page: Page,
        cx: &mut Context<Self>,
    ) -> SidebarMenuItem {
        SidebarMenuItem::new(label)
            .icon(icon)
            .active(self.page == page)
            .on_click(cx.listener(move |this, _, _, cx| this.navigate(page, cx)))
    }

    pub(crate) fn sidebar(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let menu = SidebarMenu::new().children([
            self.nav_item(c.overview, IconName::LayoutDashboard, Page::Overview, cx),
            self.nav_item(c.apps, IconName::Inbox, Page::Apps, cx),
            self.nav_item(c.processes, IconName::SquareTerminal, Page::Processes, cx),
            self.nav_item(c.cpu, IconName::Cpu, Page::Cpu, cx),
            self.nav_item(c.memory, IconName::MemoryStick, Page::Memory, cx),
            self.nav_item(c.storage, IconName::HardDrive, Page::Storage, cx),
            self.nav_item(c.network, IconName::Network, Page::Network, cx),
            self.nav_item(c.gpu, IconName::ChartPie, Page::Gpu, cx),
            self.nav_item(c.energy, IconName::Battery, Page::Energy, cx),
            self.nav_item(c.history, IconName::Calendar, Page::History, cx),
            self.nav_item(c.incidents, IconName::TriangleAlert, Page::Incidents, cx),
            self.nav_item(c.inventory, IconName::Info, Page::Inventory, cx),
            self.nav_item(c.diagnostics, IconName::CircleCheck, Page::Diagnostics, cx),
            self.nav_item(c.settings, IconName::Settings, Page::Settings, cx),
        ]);

        Sidebar::new("monitor-sidebar")
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
                            .child(Icon::new(IconName::Inspector)),
                    )
                    .when(!compact, |header| {
                        header.child(
                            v_flex()
                                .min_w_0()
                                .child(div().font_semibold().child(c.brand_name))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(c.monitor),
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
                        .child(Icon::new(IconName::CircleCheck).small())
                        .when(!compact, |row| {
                            row.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{} · {}", c.sampling, self.rounds)),
                            )
                        }),
                ),
            )
            .into_any_element()
    }

    pub(crate) fn top_bar(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let searchable = matches!(self.page, Page::Apps | Page::Processes);
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
                    .child(format!("{} {}", c.brand_name, c.monitor)),
            )
            .child(
                h_flex()
                    .min_w_0()
                    .gap_3()
                    .items_center()
                    .flex_wrap()
                    .when(searchable && !compact, |row| {
                        row.child(
                            div()
                                .w(px(300.0))
                                .max_w_full()
                                .child(Input::new(&self.filter).cleanable(true)),
                        )
                    })
                    .child(
                        Button::new("toggle-pause")
                            .when(self.paused, |button| button.primary())
                            .label(if self.paused {
                                c.resume_updates
                            } else {
                                c.pause_updates
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_pause(cx))),
                    ),
            )
            .into_any_element()
    }

    /// A bordered surface, the one container every card uses.
    pub(crate) fn surface(&self, child: impl IntoElement, cx: &mut Context<Self>) -> AnyElement {
        better_ui::surface(
            child,
            cx.theme().border,
            cx.theme().background,
            cx.theme().radius,
        )
        .into_any_element()
    }

    pub(crate) fn page_heading(&self, title: &'static str, subtitle: &'static str) -> AnyElement {
        v_flex()
            .w_full()
            .min_w_0()
            .gap_1()
            .pb_2()
            .child(div().text_2xl().font_bold().child(title))
            .child(div().min_w_0().text_sm().child(subtitle))
            .into_any_element()
    }

    /// A label with a value beneath it.
    pub(crate) fn stat(
        &self,
        label: &'static str,
        value: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        v_flex()
            .min_w(px(150.0))
            .flex_1()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(div().text_lg().min_w_0().child(value))
            .into_any_element()
    }
}

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

use crate::{app::ManagerApp, i18n::copy, model::Page};

impl ManagerApp {
    fn nav_item(
        &self,
        label: &'static str,
        icon: IconName,
        page: Page,
        suffix: Option<String>,
        cx: &mut Context<Self>,
    ) -> SidebarMenuItem {
        let item = SidebarMenuItem::new(label)
            .icon(icon)
            .active(self.page_is_active(&page));
        let item = match suffix {
            Some(suffix) => item.suffix(move |_, _| div().text_xs().child(suffix.clone())),
            None => item,
        };
        item.on_click(cx.listener(move |this, _, _, cx| {
            this.navigate(page.clone(), cx);
        }))
    }

    pub(crate) fn sidebar(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let profile = self.manager.profile();
        let profile_label = format!("{} {}", profile.distribution, profile.release);
        let architecture = profile.architecture.clone();
        let menu = SidebarMenu::new().children([
            self.nav_item(
                c.overview,
                IconName::LayoutDashboard,
                Page::Overview,
                None,
                cx,
            ),
            self.nav_item(c.components, IconName::Inbox, Page::Components, None, cx),
            self.defaults_nav_item(cx),
            self.nav_item(
                c.updates,
                IconName::ArrowDown,
                Page::Updates,
                Some(self.update_plan_count().to_string()),
                cx,
            ),
            self.nav_item(c.health, IconName::CircleCheck, Page::Health, None, cx),
            self.nav_item(c.activity, IconName::Inspector, Page::Activity, None, cx),
            self.nav_item(c.settings, IconName::Settings, Page::Settings, None, cx),
        ]);

        Sidebar::new("manager-sidebar")
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
                            .child(Icon::new(IconName::GalleryVerticalEnd)),
                    )
                    .when(!compact, |header| {
                        header.child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(div().font_semibold().child(c.brand_name))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(c.manager),
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
                        .child(Icon::new(IconName::Settings).small())
                        .when(!compact, |row| {
                            row.child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(div().text_sm().font_semibold().child(profile_label))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(architecture),
                                    ),
                            )
                        }),
                ),
            )
            .into_any_element()
    }

    pub(crate) fn top_bar(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
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
                    .child(format!("{} {}", c.brand_name, c.manager)),
            )
            .child(
                h_flex()
                    .min_w_0()
                    .gap_3()
                    .items_center()
                    .flex_wrap()
                    .when(!compact, |row| {
                        row.child(
                            div().w(px(320.0)).max_w_full().child(
                                Input::new(&self.search)
                                    .cleanable(true)
                                    .prefix(Icon::new(IconName::Search).small()),
                            ),
                        )
                    })
                    .child(
                        Button::new("update-all")
                            .primary()
                            .label(c.update_all)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.prepare_update_all(cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn page_heading(
        &self,
        title: &'static str,
        subtitle: &'static str,
        show_search: bool,
        compact: bool,
    ) -> AnyElement {
        v_flex()
            .w_full()
            .min_w_0()
            .gap_1()
            .child(div().text_2xl().font_bold().child(title))
            .child(
                div()
                    .min_w_0()
                    .text_sm()
                    .text_color(rgb(0x667085))
                    .child(subtitle),
            )
            .when(show_search && compact, |view| {
                view.child(
                    div().w_full().pt_3().child(
                        Input::new(&self.search)
                            .cleanable(true)
                            .prefix(Icon::new(IconName::Search).small()),
                    ),
                )
            })
            .into_any_element()
    }
}

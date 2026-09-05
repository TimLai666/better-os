use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, scroll::ScrollableElement, *};

use crate::{app::ManagerApp, i18n::copy, layout::COMPACT_VIEWPORT_WIDTH, model::Page};

impl ManagerApp {
    fn render_page(
        &self,
        compact: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match &self.page {
            Page::FirstRun => self.first_run_page(compact, cx),
            Page::Overview => self.overview_page(compact, cx),
            Page::Components => self.components_page(compact, cx),
            Page::ComponentDetail(_) => self.component_detail_page(compact, cx),
            Page::Defaults => self.defaults_page(compact, cx),
            Page::DefaultsComponent(_) => self.defaults_component_page(compact, cx),
            Page::DefaultsReview => self.defaults_review_page(compact, cx),
            Page::DefaultsResults => self.defaults_results_page(compact, cx),
            Page::Updates => self.updates_page(compact, cx),
            Page::ReviewChanges => self.review_changes_page(compact, cx),
            Page::Installing => self.installing_page(compact, cx),
            Page::Finished => self.finished_page(compact, cx),
            Page::Restore => self.restore_page(compact, cx),
            Page::Restored => self.restored_page(compact, cx),
            Page::Health => self.health_page(compact, cx),
            Page::DoctorResults => self.doctor_results_page(compact, cx),
            Page::Activity => self.activity_page(compact, cx),
            Page::Settings => self.settings_page(compact, window, cx),
        }
    }
}

impl ManagerApp {
    /// The shared window titlebar. Mutter gives an `xdg-toplevel` client no
    /// decorations, so this window draws its own or it has no way to be
    /// closed, minimized, maximized or moved.
    fn title_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        better_ui::window_chrome::title_bar(
            Icon::new(IconName::GalleryVerticalEnd).small(),
            format!("{} · {}", c.brand_name, c.manager),
            cx.theme().foreground,
        )
        .into_any_element()
    }
}

impl Render for ManagerApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let compact = window.viewport_size().width < px(COMPACT_VIEWPORT_WIDTH);
        let title_bar = self.title_bar(cx);

        if self.page == Page::FirstRun {
            return v_flex()
                .relative()
                .size_full()
                .child(title_bar)
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(self.first_run_page(compact, cx)),
                )
                .into_any_element();
        }

        let page = self.render_page(compact, window, cx);
        let shell = h_flex()
            .size_full()
            .min_w_0()
            .bg(cx.theme().secondary)
            .child(self.sidebar(compact, cx))
            .child(
                v_flex()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .child(self.top_bar(compact, cx))
                    // A simulation must never be mistaken for the real thing,
                    // so demo mode says so on every screen rather than only in
                    // settings.
                    .when(self.is_demo(), |this| {
                        this.child(
                            div()
                                .w_full()
                                .px_5()
                                .py_2()
                                .bg(cx.theme().warning)
                                .text_sm()
                                .text_color(cx.theme().warning_foreground)
                                .child(copy(self.locale).demo_mode_banner),
                        )
                    })
                    .child(
                        div().flex_1().min_h_0().overflow_y_scrollbar().p_5().child(
                            div()
                                .w_full()
                                .flex()
                                .justify_center()
                                .child(div().w_full().max_w(px(1320.0)).child(page)),
                        ),
                    ),
            );

        v_flex()
            .relative()
            .size_full()
            .child(title_bar)
            .child(div().flex_1().min_h_0().child(shell))
            .into_any_element()
    }
}

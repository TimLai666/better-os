use gpui::*;
use gpui_component::{ActiveTheme, scroll::ScrollableElement, *};

use crate::{app::ManagerApp, layout::COMPACT_VIEWPORT_WIDTH, model::Page};

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

impl Render for ManagerApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let compact = window.viewport_size().width < px(COMPACT_VIEWPORT_WIDTH);

        if self.page == Page::FirstRun {
            return div()
                .relative()
                .size_full()
                .child(self.first_run_page(compact, cx))
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

        div().relative().size_full().child(shell).into_any_element()
    }
}

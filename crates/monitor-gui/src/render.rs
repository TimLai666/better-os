use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{ActiveTheme, scroll::ScrollableElement, *};

use crate::app::MonitorApp;
use crate::i18n::copy;
use crate::layout::COMPACT_VIEWPORT_WIDTH;

impl Render for MonitorApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let compact = window.viewport_size().width < px(COMPACT_VIEWPORT_WIDTH);
        let c = copy(self.locale);
        let page = self.render_page(cx);

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
                    // A paused display must say so on every page. Numbers that
                    // have stopped moving are otherwise indistinguishable from
                    // a machine that has stopped doing anything.
                    .when(self.paused, |this| {
                        this.child(
                            div()
                                .w_full()
                                .px_5()
                                .py_2()
                                .bg(cx.theme().warning)
                                .text_sm()
                                .text_color(cx.theme().warning_foreground)
                                .child(c.paused_banner),
                        )
                    })
                    .child(
                        div().flex_1().min_h_0().overflow_y_scrollbar().p_5().child(
                            div()
                                .w_full()
                                .flex()
                                .justify_center()
                                .child(div().w_full().max_w(px(1440.0)).child(page)),
                        ),
                    ),
            );

        div().relative().size_full().child(shell)
    }
}

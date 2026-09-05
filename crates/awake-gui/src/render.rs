//! Where a section becomes elements. Nothing is decided here that `model.rs`
//! could decide; this file routes and lays out.

use gpui::*;
use gpui_component::{ActiveTheme, scroll::ScrollableElement, *};

use crate::{
    app::AwakeApp, i18n::copy, layout::COMPACT_VIEWPORT_WIDTH, model::Section, shell::actions,
};

impl AwakeApp {
    fn render_section(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.section {
            Section::Status => self.status_section(cx),
            Section::QuickSessions => self.quick_sessions_section(cx),
            Section::Rules => self.rules_section(window, cx),
            Section::SessionDefaults => self.session_defaults_section(cx),
            Section::Battery => self.battery_section(cx),
            Section::History => self.history_section(cx),
            Section::Diagnostics => self.diagnostics_section(cx),
            Section::Settings => self.settings_section(cx),
        }
    }
}

impl Render for AwakeApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let compact = window.viewport_size().width < px(COMPACT_VIEWPORT_WIDTH);
        let section = self.render_section(window, cx);

        div()
            .relative()
            .size_full()
            .key_context("Awake")
            .on_action(cx.listener(|this, _: &actions::ShowStatus, _, cx| {
                this.navigate(Section::Status, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::ShowQuickSessions, _, cx| {
                this.navigate(Section::QuickSessions, cx)
            }))
            .on_action(
                cx.listener(|this, _: &actions::ShowRules, _, cx| {
                    this.navigate(Section::Rules, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &actions::ShowSessionDefaults, _, cx| {
                    this.navigate(Section::SessionDefaults, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &actions::ShowBattery, _, cx| {
                this.navigate(Section::Battery, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::ShowHistory, _, cx| {
                this.navigate(Section::History, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::ShowDiagnostics, _, cx| {
                this.navigate(Section::Diagnostics, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::ShowSettings, _, cx| {
                this.navigate(Section::Settings, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::Refresh, _, cx| this.refresh(cx)))
            .v_flex()
            // Mutter gives an `xdg-toplevel` client no decorations, so this
            // window draws its own or it cannot be closed, minimized,
            // maximized or moved.
            .child(better_ui::window_chrome::title_bar(
                Icon::new(IconName::Sun).small(),
                copy(self.locale).application_name,
                cx.theme().foreground,
            ))
            .child(
                div().flex_1().min_h_0().child(
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
                                .child(
                                    div().flex_1().min_h_0().overflow_y_scrollbar().p_5().child(
                                        div()
                                            .w_full()
                                            .flex()
                                            .justify_center()
                                            .child(div().w_full().max_w(px(1320.0)).child(section)),
                                    ),
                                ),
                        ),
                ),
            )
    }
}

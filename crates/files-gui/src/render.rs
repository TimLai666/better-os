//! The frame: sidebar, chrome, content, operation center, and any dialog.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{ActiveTheme, *};

use crate::app::FilesApp;
use crate::i18n::copy;

impl Render for FilesApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The viewport is read once per frame and remembered, so a keyboard
        // page is the size of the page that is actually on screen rather than
        // a guess made when the window opened.
        self.viewport = window.viewport_size();
        let c = copy(self.locale());
        let compact = self.compact();

        let notice = self.session.notice.as_ref().map(|notice| notice.message(c));

        let toolbar = self.toolbar(cx);
        let tab_strip = self.tab_strip(cx);
        let sidebar = self.sidebar(cx);
        let content = self.content(cx);
        let operations = self
            .session
            .operations_open
            .then(|| self.operation_center(cx));
        let dialog = self.dialog(cx);
        let searching = self.session.search.is_active() || self.editing_search;
        let search_bar = searching.then(|| self.search_bar(cx));
        // The preview pane and the details panel are the same slot: both are a
        // column about the selected item, and showing two of them would leave
        // the content area a sliver on a laptop screen.
        let details = self.application_details(cx);
        let preview = (details.is_none() && self.session.preview.open && !compact)
            .then(|| self.preview_pane(cx));
        let chooser = self.chooser_overlay(cx);

        let shell = h_flex()
            .size_full()
            .min_w_0()
            .bg(cx.theme().background)
            .when(!compact, |row| row.child(sidebar))
            .child(
                v_flex()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .child(toolbar)
                    .child(tab_strip)
                    .children(search_bar)
                    .when_some(notice, |column, message| {
                        column.child(
                            div()
                                .w_full()
                                .px_4()
                                .py_1()
                                .bg(cx.theme().warning)
                                .text_sm()
                                .text_color(cx.theme().warning_foreground)
                                .child(message),
                        )
                    })
                    .child(content),
            )
            .children(preview)
            .children(details)
            .children(operations);

        div()
            .relative()
            .size_full()
            .track_focus(&self.focus_handle)
            .key_context("BetterFiles")
            .on_key_down(
                cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    this.on_key(event, window, cx)
                }),
            )
            .child(shell)
            .children(dialog)
            .children(chooser)
    }
}

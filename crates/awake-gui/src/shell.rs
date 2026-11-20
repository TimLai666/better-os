//! The frame every section is drawn inside: the sidebar, the top bar, and the
//! keyboard bindings that reach each section without a pointing device.

use better_ui::fluent::navigation::{NavItem, NavigationView};
use better_ui::icon_set::Picture;
use gpui::*;
use gpui_component::{
    ActiveTheme, StyledExt as _,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
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

/// Each section's picture in the navigation pane (ticket 147).
pub(crate) fn section_picture(section: Section) -> Picture {
    match section {
        Section::Status => Picture::Dashboard,
        Section::QuickSessions => Picture::Timer,
        Section::Rules => Picture::Automation,
        Section::SessionDefaults => Picture::Checklist,
        Section::Battery => Picture::Battery,
        Section::History => Picture::History,
        Section::Diagnostics => Picture::Diagnostics,
        Section::Settings => Picture::Settings,
    }
}

/// The section a pane index stands for: the sections in [`Section::ALL`]'s
/// order, Settings last in the pane's footer, which the kit numbers after
/// the list.
pub(crate) fn section_at(index: usize) -> Option<Section> {
    Section::ALL.get(index).copied()
}

impl AwakeApp {
    /// The kit's navigation pane, as Better Manager draws the same Keep
    /// awake pages: the sections, Settings in the footer, and `content`
    /// beside them. It follows the window's width until the person opens or
    /// closes it with its own button.
    pub(crate) fn navigation(
        &self,
        content: AnyElement,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let item = |section: Section| NavItem::new(section.title(c), section_picture(section));
        let entries = Section::ALL
            .iter()
            .filter(|section| **section != Section::Settings)
            .map(|section| item(*section).entry())
            .collect();
        let selected = Section::ALL
            .iter()
            .position(|section| *section == self.section);
        let select = cx.entity().downgrade();
        let toggle = select.clone();
        NavigationView::new("awake-nav", entries, selected)
            .footer(vec![item(Section::Settings)])
            .compact(self.nav_compact.unwrap_or(compact))
            .title(c.application_name)
            .locale(self.kit_locale())
            .on_select(move |index, _, cx| {
                if let Some(section) = section_at(index) {
                    let _ = select.update(cx, |app, cx| app.navigate(section, cx));
                }
            })
            .on_toggle(move |compact, _, cx| {
                let _ = toggle.update(cx, |app, cx| {
                    app.nav_compact = Some(compact);
                    cx.notify();
                });
            })
            .content(content)
            .into_any_element()
    }

    /// The kit's words in this window's language.
    pub(crate) fn kit_locale(&self) -> better_ui::fluent::words::Locale {
        match self.locale.resolved() {
            crate::i18n::Locale::ZhTw => better_ui::fluent::words::Locale::TraditionalChinese,
            _ => better_ui::fluent::words::Locale::English,
        }
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
                    )
                    .child(
                        h_flex()
                            .min_w_0()
                            .gap_2()
                            .items_center()
                            .child(better_ui::icon_set::picture(Picture::Keyboard, 16.0))
                            .child(
                                div()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(c.keyboard_hint),
                            ),
                    ),
            )
            .child(
                // The kit's loading ring takes the icon's place while busy.
                Button::new("refresh")
                    .primary()
                    .icon(crate::components::action_icon(
                        better_ui::icon_set::Picture::Refresh,
                        true,
                        false,
                        cx,
                    ))
                    .label(c.refresh)
                    .loading(self.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod navigation_tests {
    use std::collections::HashSet;

    use super::{section_at, section_picture};
    use crate::model::Section;

    /// The kit numbers the pane's items in order and its footer after them,
    /// so every index the pane reports names one section, Settings is the
    /// footer's, and every section has a picture of its own.
    #[test]
    fn every_pane_index_names_its_section_and_settings_is_the_footer() {
        let list = Section::ALL
            .iter()
            .filter(|section| **section != Section::Settings)
            .count();
        assert_eq!(section_at(list), Some(Section::Settings));
        for (index, section) in Section::ALL.iter().enumerate() {
            assert_eq!(section_at(index), Some(*section));
        }
        assert_eq!(section_at(Section::ALL.len()), None);
        let pictures: HashSet<_> = Section::ALL.iter().map(|s| section_picture(*s)).collect();
        assert_eq!(pictures.len(), Section::ALL.len());
    }
}

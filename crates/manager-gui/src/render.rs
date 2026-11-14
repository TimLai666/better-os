//! Draws the window: the shell around the page, and the pages ticket 84
//! builds (Home, Settings, and the placeholders for the pages to come).
//! Today's Better Manager screens are drawn by `pages_main`, `pages_flow`,
//! `pages_settings`, and `pages_defaults` inside the same content column.

use better_ui::fluent::button::Button;
use better_ui::fluent::chrome::is_back_shortcut;
use better_ui::fluent::hyperlink::HyperlinkButton;
use better_ui::fluent::info_bar::{ErrorBar, ErrorReport, InfoBar, Severity};
use better_ui::fluent::list::ListRow;
use better_ui::fluent::selection::{ComboBox, ComboItem};
use better_ui::fluent::settings_card::{DeviceBanner, SettingsCard, SettingsExpander, SettingsRow};
use better_ui::fluent::toggle::ToggleSwitch;
use better_ui::fluent::{BODY, BODY_STRONG, TITLE, label, palette, with_focus_ring};
use gpui::*;
use gpui_component::{ActiveTheme as _, IconName};
use manager_core::catalog::now_unix_seconds;

use crate::{
    app::{AppError, ManagerApp},
    attention::{self, AttentionAction},
    home::{self, QuickSetting, Tone},
    i18n::shell,
    model::{MANAGER_VERSION, Page},
    navigation::{CONTENT_MAX_WIDTH, Route},
    pages::{self, PageStatus},
    settings_page::{self, card},
};

/// One line of the content column. Every row is a direct child of the
/// scrolling column, so a search result can scroll its card into view.
struct Row {
    /// The card's id, for the search.
    card: Option<&'static str>,
    /// Space above the row: 4px between cards of one group, 8px under a
    /// group header, 24px above a group header.
    gap: Pixels,
    element: AnyElement,
}

impl Row {
    fn new(gap: f32, element: impl IntoElement) -> Self {
        Self {
            card: None,
            gap: px(gap),
            element: element.into_any_element(),
        }
    }

    fn card(gap: f32, card: &'static str, element: impl IntoElement) -> Self {
        Self {
            card: Some(card),
            ..Self::new(gap, element)
        }
    }
}

/// Space above a group header.
const GROUP_GAP: f32 = 24.0;
/// Space under a group header, before its first card.
const FIRST_CARD_GAP: f32 = 8.0;
/// Space between the cards of one group.
const CARD_GAP: f32 = 4.0;

fn severity(severity: attention::Severity) -> Severity {
    match severity {
        attention::Severity::Info => Severity::Informational,
        attention::Severity::Warning => Severity::Warning,
        attention::Severity::Error => Severity::Error,
    }
}

impl ManagerApp {
    fn title_row(&self, title: &'static str, cx: &App) -> Row {
        Row::new(
            0.0,
            label(title, TITLE, palette(cx).text_primary).pb(px(12.0)),
        )
    }

    fn group_header(&self, title: &'static str, cx: &App) -> Row {
        Row::new(
            GROUP_GAP,
            label(title, BODY_STRONG, palette(cx).text_primary),
        )
    }

    /// The banners every page carries: demo mode, and an error that stops
    /// the window from planning or saving anything.
    fn page_banners(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let mut rows = Vec::new();
        if self.is_demo() {
            rows.push(Row::new(
                0.0,
                div().pb(px(12.0)).child(
                    InfoBar::new(
                        "demo-mode",
                        Severity::Warning,
                        crate::i18n::copy(self.locale).demo_mode_banner,
                    )
                    .locale(self.locale.words()),
                ),
            ));
        }
        let built = pages::module(Route::of(&self.page)).status == PageStatus::Built;
        if built
            && matches!(
                self.planning_error,
                Some(AppError::Storage | AppError::UnsupportedHost)
            )
        {
            rows.push(Row::new(
                0.0,
                div().pb(px(12.0)).child(
                    ErrorBar::new(
                        "window-error",
                        ErrorReport::new(self.error_message(), String::new()),
                    )
                    .locale(self.locale.words()),
                ),
            ));
        }
        let _ = cx;
        rows
    }

    fn home_rows(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let s = shell(self.locale);
        let p = palette(cx);
        let mut rows = vec![self.title_row(s.home, cx)];

        if home::shows_welcome(self.state.settings.onboarding_complete) {
            rows.push(Row::new(
                0.0,
                div().pb(px(12.0)).child(
                    InfoBar::new("welcome", Severity::Informational, s.welcome_title)
                        .message(s.welcome_message)
                        .locale(self.locale.words())
                        .on_close(Self::act(cx, |this, _, cx| this.complete_onboarding(cx))),
                ),
            ));
        }

        let device = &self.device;
        let mut banner = DeviceBanner::new(
            IconName::PanelBottom,
            device
                .product
                .clone()
                .unwrap_or_else(|| s.window_title.to_string()),
        )
        .caption(home::banner_line(
            self.locale,
            device.distribution.as_deref(),
            MANAGER_VERSION,
        ));
        if let Some(battery) = device.battery {
            let icon = match battery.status {
                home::BatteryStatus::Charging => IconName::BatteryCharging,
                _ if battery.percent >= 90 => IconName::BatteryFull,
                _ if battery.percent >= 40 => IconName::BatteryMedium,
                _ => IconName::BatteryLow,
            };
            banner = banner.trailing(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .text_color(p.text_secondary)
                    .child(gpui_component::Icon::new(icon).size(px(20.0)))
                    .child(label(
                        home::battery_line(self.locale, battery),
                        BODY,
                        p.text_secondary,
                    )),
            );
        }
        rows.push(Row::new(0.0, banner));

        // Needs your attention.
        let items = pages::attention(&self.snapshot());
        rows.push(self.group_header(s.attention_header, cx));
        let mut gap = FIRST_CARD_GAP;
        if attention::everything_fine(&items) {
            rows.push(Row::new(
                gap,
                SettingsCard::new("all-fine", s.all_fine).status(Severity::Success),
            ));
            gap = CARD_GAP;
        }
        for (index, item) in items.iter().enumerate() {
            let action = item.action.clone();
            rows.push(Row::new(
                gap,
                SettingsCard::new(
                    ElementId::named_usize("attention", index),
                    item.header.clone(),
                )
                .status(severity(item.severity))
                .description(item.description.clone())
                .content(
                    Button::standard(
                        ElementId::named_usize("attention-action", index),
                        item.action_label,
                    )
                    .on_click(Self::act(
                        cx,
                        move |this, _, cx| match &action {
                            AttentionAction::Open(page) => this.navigate(page.clone(), cx),
                            AttentionAction::RefreshCatalog => this.refresh_catalog(cx),
                        },
                    )),
                ),
            ));
            gap = CARD_GAP;
        }

        // Quick settings.
        rows.push(self.group_header(s.quick_header, cx));
        for (index, quick) in home::quick_settings().into_iter().enumerate() {
            let gap = if index == 0 { FIRST_CARD_GAP } else { CARD_GAP };
            match quick {
                QuickSetting::Appearance => {
                    rows.push(Row::new(gap, self.appearance_card("home-appearance", cx)))
                }
            }
        }

        // Recent activity.
        rows.push(self.group_header(s.recent_header, cx));
        let recent = home::recent(&self.state.activity);
        if recent.is_empty() {
            rows.push(Row::new(
                FIRST_CARD_GAP,
                label(s.recent_empty, BODY, p.text_secondary),
            ));
        } else {
            let now = now_unix_seconds();
            let mut list = div().flex().flex_col().pt(px(FIRST_CARD_GAP));
            for entry in recent {
                let (icon, tone) = match home::tone(entry.kind) {
                    Tone::Success => (IconName::CircleCheck, Severity::Success),
                    Tone::Warning => (IconName::TriangleAlert, Severity::Warning),
                    Tone::Failure => (IconName::CircleX, Severity::Error),
                    Tone::Information => (IconName::Info, Severity::Informational),
                };
                let operation = entry
                    .operation
                    .map(|operation| self.operation_label(operation))
                    .unwrap_or(crate::i18n::copy(self.locale).none);
                let summary = format!(
                    "{} · {}",
                    self.component_name_for_core(entry.component.as_ref()),
                    operation
                );
                let detail = entry
                    .stage
                    .map(|stage| self.stage_label(stage).to_string())
                    .unwrap_or_else(|| {
                        crate::app::activity_evidence_text(
                            self.evidence_label(entry.evidence.as_deref(), None),
                            entry.evidence.as_deref(),
                        )
                    });
                let caption = match entry.recorded_at_unix_seconds {
                    Some(at) => format!("{detail} · {}", home::ago(self.locale, now, at)),
                    None => detail,
                };
                list = list.child(
                    ListRow::new(
                        ElementId::named_usize("recent", entry.sequence as usize),
                        summary,
                    )
                    .status_glyph(icon, tone)
                    .caption(caption),
                );
            }
            rows.push(Row::new(0.0, list));
        }
        rows.push(Row::new(
            CARD_GAP,
            div().child(
                HyperlinkButton::new("all-activity", s.recent_link)
                    .on_click(Self::act(cx, |this, _, cx| {
                        this.navigate(Page::Activity, cx)
                    })),
            ),
        ));
        rows
    }

    fn appearance_card(&self, id: &'static str, cx: &mut Context<Self>) -> SettingsCard {
        let s = shell(self.locale);
        let items = settings_page::appearance_labels(self.locale)
            .into_iter()
            .map(ComboItem::new)
            .collect();
        SettingsCard::new(id, s.appearance)
            .icon(IconName::Palette)
            .description(s.appearance_description)
            .content(
                ComboBox::new(
                    ElementId::Name(format!("{id}-choice").into()),
                    items,
                    Some(settings_page::appearance_index(self.state.settings.theme)),
                )
                .min_width(px(200.0))
                .on_select(Self::act_with(cx, |this, index: usize, window, cx| {
                    if let Some(theme) = settings_page::APPEARANCES.get(index) {
                        this.set_theme(*theme, window, cx);
                    }
                })),
            )
    }

    fn settings_rows(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let s = shell(self.locale);
        let languages = settings_page::language_labels(self.locale)
            .into_iter()
            .map(ComboItem::new)
            .collect();
        let licenses =
            settings_page::licenses_target(std::path::Path::new(settings_page::LICENSES_FILE));
        let licenses_name = settings_page::LICENSES_FILE
            .rsplit('/')
            .next()
            .unwrap_or(settings_page::LICENSES_FILE);
        let source_name = settings_page::SOURCE_URL.trim_start_matches("https://");
        vec![
            self.title_row(s.settings, cx),
            Row::card(
                0.0,
                card::APPEARANCE,
                self.appearance_card("appearance", cx),
            ),
            Row::card(
                CARD_GAP,
                card::LANGUAGE,
                SettingsCard::new("language", s.language)
                    .icon(IconName::Globe)
                    .description(s.language_description)
                    .content(
                        ComboBox::new(
                            "language-choice",
                            languages,
                            Some(settings_page::language_index(self.locale)),
                        )
                        .min_width(px(200.0))
                        .on_select(Self::act_with(
                            cx,
                            |this, index: usize, window, cx| {
                                if let Some(locale) = settings_page::LANGUAGES.get(index) {
                                    this.set_locale(*locale, window, cx);
                                }
                            },
                        )),
                    ),
            ),
            Row::card(
                CARD_GAP,
                card::CHECK_UPDATES,
                SettingsCard::new("check-updates", s.check_updates)
                    .icon(IconName::ArrowDown)
                    .content(
                        ToggleSwitch::new(
                            "check-updates-switch",
                            self.state.settings.check_updates,
                        )
                        .locale(self.locale.words())
                        .on_change(Self::act_with(cx, |this, _: bool, _, cx| {
                            this.toggle_check_updates(cx)
                        })),
                    ),
            ),
            Row::card(
                CARD_GAP,
                card::UPDATE_CHANNEL,
                SettingsCard::new("update-channel", s.update_channel)
                    .icon(IconName::Inbox)
                    .description(s.update_channel_description)
                    .content(label(
                        s.update_channel_stable,
                        BODY,
                        palette(cx).text_secondary,
                    )),
            ),
            Row::card(
                CARD_GAP,
                card::ABOUT,
                SettingsExpander::new("about", s.about, self.about_expanded)
                    .icon(IconName::Info)
                    .description(s.privacy)
                    .on_toggle(Self::act_with(cx, |this, open: bool, _, cx| {
                        this.about_expanded = open;
                        cx.notify();
                    }))
                    .row(SettingsRow::new(
                        s.about_version.replace("{version}", MANAGER_VERSION),
                    ))
                    .row(
                        SettingsRow::new(s.about_source).content(
                            HyperlinkButton::new("about-source", source_name)
                                .external(true)
                                .on_click(|_, cx| cx.open_url(settings_page::SOURCE_URL)),
                        ),
                    )
                    .row(
                        SettingsRow::new(s.about_licenses).content(
                            HyperlinkButton::new("about-licenses", licenses_name)
                                .external(true)
                                .on_click(move |_, cx| cx.open_url(&licenses)),
                        ),
                    ),
            ),
        ]
    }

    /// A page a later ticket builds: its title, and in a debug build the
    /// "not built yet" card naming the ticket.
    fn placeholder_rows(&self, route: Route, ticket: u16, cx: &mut Context<Self>) -> Vec<Row> {
        let s = shell(self.locale);
        let mut rows = vec![self.title_row(pages::title(route, self.locale), cx)];
        if pages::shows_not_built_notice() {
            rows.push(Row::new(
                0.0,
                SettingsCard::new("not-built", s.not_built_title)
                    .status(Severity::Informational)
                    .description(s.not_built_body.replace("{ticket}", &ticket.to_string())),
            ));
        }
        rows
    }

    /// Today's Better Manager screens, unchanged, as one row.
    fn interim_rows(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Row> {
        let compact = crate::navigation::legacy_compact(f32::from(window.viewport_size().width));
        let mut rows = Vec::new();
        if self.page == Page::Components {
            // Apps' sub-page cards, above today's component list, so the
            // update check, the doctor, and the activity log stay reachable
            // until ticket 85 rebuilds the page.
            rows.push(self.title_row(shell(self.locale).apps, cx));
            let links = pages::apps_links(self.locale, self.snapshot().update_count());
            for (index, link) in links.into_iter().enumerate() {
                let page = link.page.clone();
                rows.push(Row::card(
                    if index == 0 { 0.0 } else { CARD_GAP },
                    link.card,
                    SettingsCard::new(link.card, link.header)
                        .icon(match link.page {
                            Page::Updates => IconName::ArrowDown,
                            Page::Health => IconName::CircleCheck,
                            _ => IconName::Inspector,
                        })
                        .description(link.description)
                        .on_click(Self::act(cx, move |this, _, cx| {
                            this.navigate(page.clone(), cx)
                        })),
                ));
            }
        }
        let element = match &self.page {
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
            _ => div().into_any_element(),
        };
        rows.push(Row::new(
            if rows.is_empty() { 0.0 } else { GROUP_GAP },
            element,
        ));
        rows
    }

    fn page_rows(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Row> {
        let route = Route::of(&self.page);
        match (&self.page, pages::module(route).status) {
            (Page::Home, _) => self.home_rows(cx),
            (Page::Settings, _) => self.settings_rows(cx),
            (_, PageStatus::Placeholder { ticket }) => self.placeholder_rows(route, ticket, cx),
            _ => self.interim_rows(window, cx),
        }
    }

    /// The scrolling content column: at most 1000px wide, 24px from the
    /// pane.
    fn content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let mut rows = self.page_banners(cx);
        rows.extend(self.page_rows(window, cx));

        // A search result's card: scroll it into view and give it the
        // keyboard, once.
        let pending = self.pending_card.take();
        let target = self.focused_card;
        let ring = self.card_focus.is_focused(window) && window.last_input_was_keyboard();
        let mut column = div()
            .id("content")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.content_scroll)
            .flex()
            .flex_col()
            .pl(px(24.0))
            .pr(px(24.0))
            .pt(px(12.0))
            .pb(px(40.0));
        for (index, row) in rows.into_iter().enumerate() {
            let mut wrapper = div()
                .w_full()
                .max_w(px(CONTENT_MAX_WIDTH))
                .flex_none()
                .pt(row.gap);
            if row.card.is_some() && row.card == target {
                if pending.is_some() {
                    self.content_scroll.scroll_to_item(index);
                    self.card_focus.focus(window, cx);
                }
                wrapper = wrapper.child(with_focus_ring(
                    div()
                        .id("focused-card")
                        .relative()
                        .track_focus(&self.card_focus)
                        .child(row.element),
                    ring || pending.is_some(),
                    better_ui::fluent::radius::SM,
                    p,
                ));
            } else {
                wrapper = wrapper.child(row.element);
            }
            column = column.child(wrapper);
        }
        column.into_any_element()
    }
}

impl Render for ManagerApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.dark = cx.theme().is_dark();
        let p = palette(cx);
        let content = self.content(window, cx);
        let navigation = self.navigation(content, window, cx);
        let title_bar = self.title_bar(cx);
        div()
            .id("control-center")
            .track_focus(&self.root_focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            // The window frame paints the window background, with the
            // desktop's top corners (ticket 108).
            .text_color(p.text_primary)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let keystroke = &event.keystroke;
                let m = &keystroke.modifiers;
                if is_back_shortcut(&keystroke.key, m.control, m.alt, m.shift, m.platform) {
                    cx.stop_propagation();
                    this.go_back(cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _, _, cx| this.go_back(cx)),
            )
            .child(title_bar)
            .child(div().flex_1().min_h_0().child(navigation))
            .children(self.update_restart.toast_layer(self.locale.words(), cx))
    }
}

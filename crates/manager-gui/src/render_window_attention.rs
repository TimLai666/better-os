//! Draws the Window attention page (ticket 106) from what
//! `window_attention_page` decides, with the Fluent kit only.

use better_ui::AppIcon;
use better_ui::fluent::auto_suggest::{AutoSuggestBox, Suggestion};
use better_ui::fluent::button::Button;
use better_ui::fluent::info_bar::{ErrorBar, ErrorReport, Severity};
use better_ui::fluent::selection::{ComboBox, ComboItem};
use better_ui::fluent::settings_card::SettingsCard;
use better_ui::fluent::{BODY, label, palette};
use gpui::*;
use gpui_component::IconName;

use crate::app::ManagerApp;
use crate::i18n::shell;
use crate::render::{CARD_GAP, FIRST_CARD_GAP, Row};
use crate::window_attention_page::{
    self as page, ExtensionState, NoticeAction, card, extension_notice, service_notice,
};

/// The width of every list on the page, so they line up: enough for the
/// longest item in either language ("Use the default (Bring the window
/// forward)"). A longer translation still widens its own list rather than
/// being cut.
const LIST_WIDTH: f32 = 330.0;

impl ManagerApp {
    pub(crate) fn window_attention_rows(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let locale = self.locale;
        let w = page::copy(locale);
        let state = &self.window_attention;
        let mut rows = vec![
            self.title_row(shell(locale).window_attention, cx),
            Row::new(
                0.0,
                label(w.default_description, BODY, palette(cx).text_secondary).pb(px(16.0)),
            ),
        ];

        if let Some((summary, details)) = &state.failure {
            rows.push(Row::new(
                0.0,
                div().pb(px(12.0)).child(
                    ErrorBar::new(
                        "attention-failure",
                        ErrorReport::new(*summary, details.clone()),
                    )
                    .locale(locale.words()),
                ),
            ));
        }

        // The extension first: until it is on, the page offers to turn it on
        // and shows no setting (the ticket's rule).
        if let Some(notice) = extension_notice(locale, &state.extension) {
            let severity = match state.extension {
                ExtensionState::Failed(_) => Severity::Error,
                ExtensionState::NotInstalled
                | ExtensionState::Unsupported
                | ExtensionState::NoShell => Severity::Warning,
                _ => Severity::Informational,
            };
            let mut notice_card =
                SettingsCard::new("attention-extension", notice.title).status(severity);
            if !notice.body.is_empty() {
                notice_card = notice_card.description(notice.body.clone());
            }
            if notice.action == Some(NoticeAction::TurnOn) {
                notice_card = notice_card.content(
                    Button::accent(
                        "attention-turn-on",
                        if state.turning_on {
                            w.turning_on
                        } else {
                            w.turn_on
                        },
                    )
                    .disabled(state.turning_on)
                    .on_click(Self::act(cx, |this, _, cx| this.turn_on_shell_adapter(cx))),
                );
            }
            rows.push(Row::new(0.0, notice_card));
            if !notice.settings_shown {
                return rows;
            }
        }

        let choices = match &state.choices {
            None => return rows,
            Some(Err(details)) => {
                rows.push(Row::new(
                    0.0,
                    ErrorBar::new(
                        "attention-read-failed",
                        ErrorReport::new(
                            w.read_failed,
                            format!("{}\n{details}", state.store.choices_path().display()),
                        )
                        .advice(w.read_failed_advice),
                    )
                    .locale(locale.words())
                    .expanded(true),
                ));
                return rows;
            }
            Some(Ok(choices)) => choices,
        };

        // The service's state is a card like the extension's: an icon, the
        // title, what it means, and the one thing to do about it.
        if let Some(notice) = service_notice(locale, &state.service) {
            let mut service_card = SettingsCard::new("attention-service", notice.title)
                .status(Severity::Warning)
                .description(notice.body.clone());
            if notice.action == Some(NoticeAction::StartService) {
                service_card = service_card.content(
                    Button::standard("attention-start-service", w.start_service).on_click(
                        Self::act(cx, |this, _, cx| this.refresh_window_attention(cx)),
                    ),
                );
            }
            rows.push(Row::new(0.0, div().pb(px(12.0)).child(service_card)));
        }

        let default_items = page::default_items(locale)
            .into_iter()
            .map(ComboItem::new)
            .collect();
        rows.push(Row::card(
            0.0,
            card::DEFAULT,
            SettingsCard::new(card::DEFAULT, w.default_header)
                .icon(IconName::Bell)
                .description(page::choice_explanation(locale, choices.default))
                .content(
                    ComboBox::new(
                        "attention-default-choice",
                        default_items,
                        Some(page::default_index(choices.default)),
                    )
                    .min_width(px(LIST_WIDTH))
                    .on_select(Self::act_with(cx, |this, index: usize, _, cx| {
                        this.set_attention_default(index, cx)
                    })),
                ),
        ));

        // Apps: the add box, then each app.
        rows.push(self.group_header(w.apps_header, cx));
        let listed = state.rows();
        let query = self.window_attention_add.read(cx).value().to_string();
        let found = page::suggestions(&state.installed, &listed, &query);
        let suggestions = found
            .iter()
            .map(|app| Suggestion::new(app.name.clone()).caption(app.id.to_string()))
            .collect();
        let input = self.window_attention_add.clone();
        let mut add_card = SettingsCard::new(card::ADD, w.add_header)
            .icon(IconName::Plus)
            .content(
                AutoSuggestBox::new("attention-add-box", &self.window_attention_add)
                    .width(px(LIST_WIDTH))
                    .suggestions(suggestions)
                    .empty_text(w.add_no_result.replace("{query}", query.trim()))
                    .on_choose(Self::act_with(cx, move |this, index: usize, window, cx| {
                        if let Some(app) = found.get(index).cloned() {
                            this.add_attention_app(app, cx);
                            input.update(cx, |input, cx| input.set_value("", window, cx));
                        }
                    })),
            );
        if listed.is_empty() {
            add_card = add_card.description(w.apps_empty);
        }
        rows.push(Row::card(FIRST_CARD_GAP, card::ADD, add_card));

        let items: Vec<String> = page::app_items(locale, choices.default);
        for (index, row) in listed.into_iter().enumerate() {
            let status = if row.own.is_some() {
                w.own_choice
            } else {
                w.follows_default
            };
            let selected = page::app_index(row.own);
            let picked = row.clone();
            rows.push(Row::new(
                CARD_GAP,
                SettingsCard::new(
                    ElementId::named_usize("attention-app", index),
                    row.name.clone(),
                )
                .leading(app_icon(state.icons.get(&row.app), &row.name, cx))
                .description(format!("{status} · {}", row.app))
                .content(
                    ComboBox::new(
                        ElementId::named_usize("attention-app-choice", index),
                        items.iter().cloned().map(ComboItem::new).collect(),
                        Some(selected),
                    )
                    .min_width(px(LIST_WIDTH))
                    .on_select(Self::act_with(
                        cx,
                        move |this, choice: usize, _, cx| {
                            this.set_attention_for_app(picked.clone(), choice, cx)
                        },
                    )),
                ),
            ));
        }
        rows
    }
}

/// An app's own icon at the card glyph's size, or its first letter when the
/// app has none or is no longer installed.
fn app_icon(icon: Option<&AppIcon>, name: &str, cx: &App) -> AnyElement {
    let p = palette(cx);
    icon.cloned()
        .unwrap_or_else(|| AppIcon::Letter(better_ui::icons::initial(name)))
        .element(
            better_ui::fluent::settings_card::LEADING_SIZE,
            p.text_on_accent.into(),
            p.accent.into(),
            better_ui::fluent::radius::SM,
        )
}

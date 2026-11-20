//! Draws Fixes with the Fluent kit. Nothing here decides anything: `model`
//! says what each row is, and every button goes through `state`.

use better_ui::fluent::button::Button;
use better_ui::fluent::command_bar::{Command, CommandBar};
use better_ui::fluent::info_bar::{ErrorBar, ErrorReport, InfoBar, Severity};
use better_ui::fluent::progress::{Progress, ProgressRing};
use better_ui::fluent::settings_card::{SettingsExpander, SettingsRow};
use better_ui::fluent::{BODY, CAPTION, Palette, label, palette};
use gpui::*;
use gpui_component::IconName;

use super::card;
use super::copy::copy;
use super::model::{FaultRow, KnownRow, RowAction, Tone, page_rows};
use super::state::Work;
use crate::app::ManagerApp;
use crate::i18n::shell;
use crate::render::{CARD_GAP, FIRST_CARD_GAP, Row};

fn tone_color(tone: Tone, p: &Palette) -> Rgba {
    match tone {
        Tone::Neutral => p.text_secondary,
        Tone::Success => p.success,
        Tone::Warning => p.caution,
    }
}

fn id_for(prefix: &str, key: &super::model::RowKey) -> ElementId {
    let subject = match &key.subject {
        patch_core::Subject::Machine => "machine".to_string(),
        patch_core::Subject::FlatpakApp(app) => app.to_string(),
    };
    ElementId::Name(format!("{prefix}-{:?}-{subject}", key.entry).into())
}

impl ManagerApp {
    /// The page.
    pub(crate) fn fixes_rows(&mut self, cx: &mut Context<Self>) -> Vec<Row> {
        let locale = self.locale;
        let c = copy(locale);
        let s = shell(locale);
        let p = palette(cx);
        let mut rows = vec![self.title_row(s.fixes, cx)];

        let busy = self.fixes.checking() || self.fixes.work.is_some();
        let last_checked = self.fixes.checked_at.map(|at| {
            c.last_checked.replace(
                "{time}",
                &crate::home::ago(locale, manager_core::catalog::now_unix_seconds(), at),
            )
        });
        rows.push(Row::card(
            0.0,
            card::CHECK,
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .pb(px(8.0))
                .child(label(c.description, BODY, p.text_secondary))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(12.0))
                        .child(
                            CommandBar::new("fixes-commands")
                                .locale(locale.words())
                                .command(
                                    Command::new(c.check_again, IconName::Redo)
                                        .disabled(busy)
                                        .on_click(Self::act(cx, |this, _, cx| {
                                            this.check_fixes_again(cx)
                                        })),
                                ),
                        )
                        .children(last_checked.map(|line| label(line, CAPTION, p.text_secondary))),
                ),
        ));

        if self.fixes.unavailable {
            rows.push(Row::new(
                0.0,
                div().pb(px(12.0)).child(
                    InfoBar::new("fixes-unavailable", Severity::Warning, c.host_unavailable)
                        .locale(locale.words()),
                ),
            ));
        }
        if let Some(record_error) = &self.fixes.record_error {
            rows.push(Row::new(
                0.0,
                div().pb(px(12.0)).child(
                    ErrorBar::new(
                        "fixes-record",
                        ErrorReport::new(c.record_unreadable, record_error.clone()),
                    )
                    .locale(locale.words()),
                ),
            ));
        }
        if let Some(failure) = &self.fixes.failure {
            rows.push(Row::new(
                0.0,
                div().pb(px(12.0)).child(
                    ErrorBar::new(
                        "fixes-failure",
                        ErrorReport::new(
                            crate::pages::states::describe(
                                locale,
                                &crate::pages::states::PageState::WriteFailed {
                                    reason: failure.summary.to_string(),
                                },
                            )
                            .text,
                            failure.details.clone(),
                        ),
                    )
                    .locale(locale.words()),
                ),
            ));
        }
        if self.fixes.shows_ring() {
            rows.push(Row::new(
                0.0,
                div()
                    .pb(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        ProgressRing::new("fixes-checking", Progress::Indeterminate).size(px(32.0)),
                    )
                    .child(label(c.checking, CAPTION, p.text_secondary)),
            ));
        }
        let Some(reports) = self.fixes.reports.as_ref() else {
            return rows;
        };
        let groups = page_rows(
            locale,
            reports,
            &self.fixes.results,
            &self.fixes_override_dir(),
        );

        rows.push(self.group_header(c.group_found, cx));
        if groups.found.is_empty() {
            rows.push(Row::card(
                FIRST_CARD_GAP,
                card::FOUND,
                label(c.nothing_found, BODY, p.text_secondary),
            ));
        } else {
            for (index, row) in groups.found.iter().enumerate() {
                let expander = self.fault_expander(row, cx);
                rows.push(if index == 0 {
                    Row::card(FIRST_CARD_GAP, card::FOUND, expander)
                } else {
                    Row::new(CARD_GAP, expander)
                });
            }
        }

        if crate::pages::states::draws_group_header(groups.applied.len()) {
            rows.push(self.group_header(c.group_applied, cx));
            for (index, row) in groups.applied.iter().enumerate() {
                let expander = self.fault_expander(row, cx);
                rows.push(if index == 0 {
                    Row::card(FIRST_CARD_GAP, card::APPLIED, expander)
                } else {
                    Row::new(CARD_GAP, expander)
                });
            }
        }

        rows.push(self.group_header(c.group_all, cx));
        for (index, row) in groups.all.iter().enumerate() {
            let gap = if index == 0 { FIRST_CARD_GAP } else { CARD_GAP };
            let card_id = match row.entry {
                patch_core::EntryId::FontconfigCache => card::FONT_CACHE,
                patch_core::EntryId::ElectronCursor => card::ELECTRON_CURSOR,
            };
            rows.push(Row::card(gap, card_id, self.known_expander(row, cx)));
        }
        rows
    }

    /// One fault: the symptom, the cause, its state and buttons, and,
    /// expanded, the exact change.
    fn fault_expander(&self, row: &FaultRow, cx: &mut Context<Self>) -> SettingsExpander {
        let locale = self.locale;
        let c = copy(locale);
        let p = palette(cx);
        let open = self.fixes.open.contains(&row.key);
        let can_change = self.fixes.can_change();
        let working = match &self.fixes.work {
            Some(Work::Fixing(key)) if key == &row.key => Some(c.fixing),
            Some(Work::Undoing(key)) if key == &row.key => Some(c.undoing),
            _ => None,
        };
        let mut header = div().flex().flex_wrap().items_center().gap(px(8.0));
        match working {
            Some(words) => {
                header = header
                    .child(
                        ProgressRing::new(
                            id_for("fixes-working", &row.key),
                            Progress::Indeterminate,
                        )
                        .size(px(16.0)),
                    )
                    .child(label(words, CAPTION, p.text_secondary));
            }
            None => {
                header = header.child(label(
                    row.state.0.clone(),
                    CAPTION,
                    tone_color(row.state.1, p),
                ));
            }
        }
        for (index, action) in row.actions.iter().enumerate() {
            let text = match action {
                RowAction::Fix => c.fix,
                RowAction::FixAgain => c.fix_again,
                RowAction::Undo => c.undo,
                RowAction::UndoAll => c.undo_all,
            };
            let id = id_for(&format!("fixes-action-{index}"), &row.key);
            let button = if index == 0 && matches!(action, RowAction::Fix | RowAction::FixAgain) {
                Button::accent(id, text)
            } else {
                Button::standard(id, text)
            };
            let key = row.key.clone();
            let action = *action;
            header = header.child(button.disabled(!can_change).on_click(Self::act(
                cx,
                move |this, _, cx| match action {
                    RowAction::Fix | RowAction::FixAgain => this.fix_fault(key.clone(), cx),
                    RowAction::Undo => this.undo_fix(key.clone(), false, cx),
                    RowAction::UndoAll => this.undo_fix(key.clone(), true, cx),
                },
            )));
        }
        let icon = match row.state.1 {
            Tone::Success => IconName::CircleCheck,
            Tone::Neutral => IconName::Info,
            Tone::Warning => IconName::TriangleAlert,
        };
        let toggle_key = row.key.clone();
        let mut expander =
            SettingsExpander::new(id_for("fixes-fault", &row.key), row.header.clone(), open)
                .icon(icon)
                .description(row.cause)
                .content(header)
                .on_toggle(Self::act_with(cx, move |this, open: bool, _, cx| {
                    if open {
                        this.fixes.open.insert(toggle_key.clone());
                    } else {
                        this.fixes.open.remove(&toggle_key);
                    }
                    cx.notify();
                }));
        for line in &row.change {
            expander = expander.row(SettingsRow::new(line.clone()));
        }
        if let Some(when) = &row.takes_effect {
            expander = expander.row(SettingsRow::new(when.clone()));
        }
        expander = expander.row(SettingsRow::new(row.undo_note));
        for line in &row.changed_elsewhere {
            expander = expander.row(SettingsRow::new(line.clone()).content(label(
                crate::i18n::copy(locale).state_changed_externally,
                CAPTION,
                p.caution,
            )));
        }
        for limit in &row.limits {
            expander = expander.row(SettingsRow::new(*limit));
        }
        expander
    }

    /// One entry on "All known problems": its symptom, its state, and what
    /// it can and cannot do.
    fn known_expander(&self, row: &KnownRow, cx: &mut Context<Self>) -> SettingsExpander {
        let open = self.fixes.known_open.contains(&row.entry);
        let entry = row.entry;
        let mut expander = SettingsExpander::new(
            ElementId::Name(format!("fixes-known-{entry:?}").into()),
            row.header,
            open,
        )
        .icon(IconName::Info)
        .description(row.state.0.clone())
        .on_toggle(Self::act_with(cx, move |this, open: bool, _, cx| {
            if open {
                this.fixes.known_open.insert(entry);
            } else {
                this.fixes.known_open.remove(&entry);
            }
            cx.notify();
        }))
        .row(SettingsRow::new(row.cause));
        for limit in &row.limits {
            expander = expander.row(SettingsRow::new(*limit));
        }
        expander
    }
}

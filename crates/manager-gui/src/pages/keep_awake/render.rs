//! Draws Keep awake (spec section 6) from what `model` and `decide` say, with
//! the Fluent kit only.

use awake_core::{Combine, Condition, ProcessMatchKind, ProviderKind, Weekday};
use awake_ipc::{QuickSessions, WireRemaining};
use better_ui::fluent::breadcrumb::BreadcrumbBar;
use better_ui::fluent::button::{Button, ButtonKind, DropDownButton, SplitButton};
use better_ui::fluent::command_bar::{Command, CommandBar};
use better_ui::fluent::dialog::{ContentDialog, DefaultButton, DialogButtons, DialogResult};
use better_ui::fluent::info_bar::{ErrorBar, ErrorReport, InfoBar, Severity};
use better_ui::fluent::list::{Column, DetailsTable, SortState, TableCell};
use better_ui::fluent::menu::{MenuEntry, MenuItem};
use better_ui::fluent::progress::{Progress, ProgressRing};
use better_ui::fluent::selection::{ComboBox, ComboItem};
use better_ui::fluent::settings_card::{SettingsCard, SettingsExpander, SettingsRow};
use better_ui::fluent::tabs::{SelectorBar, SelectorItem};
use better_ui::fluent::text_box::{NumberBox, NumberModel, TextBox};
use better_ui::fluent::time_picker::{Time, TimeModel, TimePicker};
use better_ui::fluent::toggle::ToggleSwitch;
use better_ui::fluent::{BODY, CAPTION, label, palette};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::IconName;

use super::card;
use super::copy::{Copy, copy, fill, page_copy};
use super::decide::{
    self, DefaultField, Headline, LID_LIMITS, ReasonAction, Service, SubPage, length_label,
};
use super::model::{
    BatteryView, ConditionView, ProviderRow, RuleTestView, StatusView, provider_label,
    remaining_label, started_label, suppression_label,
};
use super::state::{COMPONENT, Dialog};
use crate::app::ManagerApp;
use crate::i18n::shell;
use crate::pages::states::{Drawn, PageState, describe};
use crate::render::{CARD_GAP, FIRST_CARD_GAP, GROUP_GAP, Row};

/// What choosing "all" or "any" does in the rule editor.
type PickCombine = Box<dyn Fn(Combine, &mut ManagerApp, &mut Context<ManagerApp>)>;

/// The width of the page's lists and boxes, so they line up.
const CONTROL_WIDTH: f32 = 200.0;

impl ManagerApp {
    pub(crate) fn keep_awake_rows(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Row> {
        let locale = self.locale;
        let w = page_copy(locale);
        let service = self.keep_awake_service();
        let mut rows = Vec::new();

        if self.keep_awake.draft.is_some() && service.is_ready() {
            let crumb = if self
                .keep_awake
                .draft
                .as_ref()
                .is_some_and(|draft| draft.rule_id.is_some())
            {
                w.edit_rule_crumb
            } else {
                w.new_rule_crumb
            };
            rows.push(Row::new(
                0.0,
                div().pb(px(12.0)).child(
                    BreadcrumbBar::new(
                        "awake-crumbs",
                        vec![shell(locale).keep_awake.into(), crumb.into()],
                    )
                    .on_click(Self::act_with(
                        cx,
                        |this, index: usize, _, cx| {
                            if index == 0 {
                                this.close_draft(cx);
                            }
                        },
                    )),
                ),
            ));
            rows.extend(self.failure_rows(cx));
            rows.extend(self.rule_editor_rows(window, cx));
            return rows;
        }

        rows.push(self.title_row(shell(locale).keep_awake, cx));
        match &service {
            Service::Reading => {
                if self.keep_awake.shows_ring() {
                    let text = describe(locale, &PageState::Loading { elapsed_ms: 300 }).text;
                    rows.push(Row::new(
                        0.0,
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .child(
                                ProgressRing::new("awake-reading", Progress::Indeterminate)
                                    .size(px(32.0)),
                            )
                            .child(label(text, CAPTION, palette(cx).text_secondary)),
                    ));
                }
                return rows;
            }
            Service::NotInstalled | Service::TooOld { .. } => {
                let state = match &service {
                    Service::TooOld { installed } => PageState::TooOld {
                        component: "Better Awake".to_string(),
                        installed: installed.clone(),
                        required: crate::model::MANAGER_VERSION.to_string(),
                    },
                    _ => PageState::NotInstalled {
                        component: "Better Awake".to_string(),
                    },
                };
                let words = describe(locale, &state);
                let mut state_card =
                    SettingsCard::new("awake-component", words.text).icon(IconName::Sun);
                if let Some(action) = words.action {
                    state_card = state_card.content(
                        Button::accent("awake-component-action", action).on_click(Self::act(
                            cx,
                            |this, _, cx| {
                                if let Ok(id) = better_core::ComponentId::new(COMPONENT) {
                                    this.prepare_component_change(&id, cx);
                                }
                            },
                        )),
                    );
                }
                rows.push(Row::new(0.0, state_card));
                return rows;
            }
            Service::NotRunning {
                offers_start,
                details,
            } => {
                let words = describe(
                    locale,
                    &PageState::ServiceNotRunning {
                        component: "Better Awake".to_string(),
                    },
                );
                debug_assert_eq!(words.drawn, Drawn::WarningBar);
                let mut bar = InfoBar::new("awake-stopped", Severity::Warning, words.text)
                    .locale(locale.words());
                if *offers_start && let Some(action) = words.action {
                    bar = bar.action(Button::standard("awake-start-service", action).on_click(
                        Self::act(cx, |this, _, cx| this.start_keep_awake_service(cx)),
                    ));
                }
                rows.push(Row::new(0.0, div().pb(px(12.0)).child(bar)));
                if !details.is_empty() {
                    rows.push(Row::new(
                        0.0,
                        div().pb(px(12.0)).child(
                            ErrorBar::new(
                                "awake-read-failed",
                                ErrorReport::new(w.read_failed, details.clone()),
                            )
                            .locale(locale.words()),
                        ),
                    ));
                }
                // The sub-pages stay where they are, and stay still: nothing
                // here can be read or changed until the service runs (A3).
                rows.push(Row::new(0.0, self.selector(false, cx)));
                return rows;
            }
            Service::Ready => {}
        }

        rows.extend(self.failure_rows(cx));
        rows.push(Row::new(
            0.0,
            div().pb(px(8.0)).child(self.selector(true, cx)),
        ));
        match self.keep_awake.sub {
            SubPage::Status => rows.extend(self.awake_status_rows(window, cx)),
            SubPage::Rules => rows.extend(self.awake_rules_rows(window, cx)),
            SubPage::History => rows.extend(self.awake_history_rows(cx)),
            SubPage::Diagnostics => rows.extend(self.awake_diagnostics_rows(cx)),
        }
        rows
    }

    fn failure_rows(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let Some((summary, details)) = &self.keep_awake.failure else {
            return Vec::new();
        };
        vec![Row::new(
            0.0,
            div().pb(px(12.0)).child(
                ErrorBar::new("awake-failure", ErrorReport::new(*summary, details.clone()))
                    .locale(self.locale.words())
                    .on_close(Self::act(cx, |this, _, cx| {
                        this.keep_awake.failure = None;
                        cx.notify();
                    })),
            ),
        )]
    }

    fn selector(&self, live: bool, cx: &mut Context<Self>) -> SelectorBar {
        let w = page_copy(self.locale);
        let items = SubPage::ALL
            .iter()
            .map(|page| {
                SelectorItem::new(page.title(w))
                    .tooltip(fill(w.shortcut_hint, "key", &page.shortcut()))
                    .disabled(!live)
            })
            .collect();
        SelectorBar::new("awake-selector", items, Some(self.keep_awake.sub.index())).on_select(
            Self::act_with(cx, |this, index: usize, _, cx| {
                if let Some(page) = SubPage::ALL.get(index) {
                    this.set_keep_awake_sub(*page, cx);
                }
            }),
        )
    }

    // ---- Status -----------------------------------------------------------

    fn awake_status_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Row> {
        let locale = self.locale;
        let c = copy(locale);
        let w = page_copy(locale);
        let p = palette(cx);
        let Some(status) = self.keep_awake.status().cloned() else {
            return Vec::new();
        };
        let view = StatusView::from_status(&status);
        let offset = self.keep_awake.offset();
        let offer_document = self.keep_awake.offer().cloned();
        let offer = offer_document
            .as_ref()
            .map(|document| document.quick_sessions.clone())
            .unwrap_or_default();
        let busy = self.keep_awake.busy;
        let mut rows = Vec::new();

        // The top card: the state, and the one control it calls for.
        let headline = decide::Headline::of(&status, &view);
        let (header, description) = headline.words(w, c, offset);
        let mut top = SettingsCard::new(card::STATE, header).icon(IconName::Sun);
        if !description.is_empty() {
            top = top.description(description);
        }
        top = top.content(self.headline_control(&headline, &offer, busy, cx));
        rows.push(Row::card(0.0, card::STATE, top));
        if let Some(reason) = &view.interrupted_previous_session {
            rows.push(Row::new(
                CARD_GAP,
                InfoBar::new(
                    "awake-interrupted",
                    Severity::Informational,
                    c.interrupted_previous_session,
                )
                .message(reason.clone())
                .locale(locale.words()),
            ));
        }
        if let Some(document) = &offer_document
            && let Some(notice) = decide::import_notice(document, w)
        {
            rows.push(Row::new(
                CARD_GAP,
                InfoBar::new("awake-import", Severity::Warning, notice).locale(locale.words()),
            ));
        }

        // Active reasons.
        if !view.reasons.is_empty() {
            rows.push(self.group_header(w.active_reasons_group, cx));
            for (index, reason) in view.reasons.iter().enumerate() {
                let session_id = reason.session_id;
                let remaining = reason
                    .remaining
                    .map(|remaining| remaining_label(remaining, c))
                    .unwrap_or_default();
                let started = reason
                    .started_at_unix_seconds
                    .map(|at| started_label(at, offset))
                    .unwrap_or_default();
                let mut line = format!(
                    "{} · {}",
                    reason.origin_label(c),
                    fill(
                        &fill(w.reason_line, "started", &started),
                        "remaining",
                        &remaining
                    )
                );
                let pending = self.keep_awake.pending_end == Some(session_id);
                if pending {
                    line = format!("{line}\n{}", view.ending_explanation(session_id, c));
                }
                let mut reason_card = SettingsCard::new(
                    ElementId::named_usize("awake-reason", index),
                    reason.display_name().to_string(),
                )
                .icon(match reason.origin {
                    awake_core::SessionOrigin::Manual => IconName::User,
                    awake_core::SessionOrigin::Trigger => IconName::Bot,
                })
                .description(line);
                reason_card = if pending {
                    reason_card.content(
                        div()
                            .flex()
                            .gap(px(8.0))
                            .child(
                                Button::accent(
                                    ElementId::named_usize("awake-end-confirm", index),
                                    w.end,
                                )
                                .disabled(busy)
                                .on_click(Self::act(cx, move |this, _, cx| {
                                    this.end_keep_awake_reason(session_id, cx)
                                })),
                            )
                            .child(
                                Button::standard(
                                    ElementId::named_usize("awake-end-cancel", index),
                                    w.cancel,
                                )
                                .on_click(Self::act(cx, |this, _, cx| {
                                    this.ask_end_keep_awake_reason(None, cx)
                                })),
                            ),
                    )
                } else {
                    let entries = decide::reason_actions(reason)
                        .into_iter()
                        .map(|action| {
                            MenuItem::new(decide::reason_action_label(action, w))
                                .on_select(Self::act(cx, move |this, _, cx| match action {
                                    ReasonAction::Extend(minutes) => {
                                        this.extend_keep_awake(session_id, minutes, cx)
                                    }
                                    ReasonAction::Modify => this.open_modify(session_id, cx),
                                    ReasonAction::End => {
                                        this.ask_end_keep_awake_reason(Some(session_id), cx)
                                    }
                                }))
                                .entry()
                        })
                        .collect();
                    reason_card.content(
                        DropDownButton::more(
                            ElementId::named_usize("awake-reason-more", index),
                            w.more_options,
                        )
                        .entries(entries)
                        .disabled(busy)
                        .align_right(),
                    )
                };
                rows.push(Row::new(
                    if index == 0 { FIRST_CARD_GAP } else { CARD_GAP },
                    reason_card,
                ));
            }
        }

        // What's in effect.
        rows.push(self.group_header(w.in_effect_group, cx));
        for (index, row) in view.policy.iter().enumerate() {
            let caution = row.prevented && !row.delivered;
            let value = row.value(c);
            rows.push(Row::new(
                if index == 0 { FIRST_CARD_GAP } else { CARD_GAP },
                SettingsCard::new(
                    ElementId::named_usize("awake-policy", index),
                    row.field.label(c),
                )
                .content(label(
                    value,
                    BODY,
                    if caution { p.caution } else { p.text_secondary },
                )),
            ));
        }
        if view.lid.held {
            rows.push(Row::new(
                CARD_GAP,
                SettingsCard::new("awake-lid-countdown", c.lid_limit_heading).content(label(
                    view.lid
                        .limit_summary(locale, status.now_unix_seconds, offset),
                    BODY,
                    p.text_secondary,
                )),
            ));
        }
        let mut conflicts = SettingsExpander::new(
            "awake-conflicts",
            w.conflicts_expander,
            self.keep_awake.conflicts_open,
        )
        .on_toggle(Self::act_with(cx, |this, open: bool, _, cx| {
            this.keep_awake.conflicts_open = open;
            cx.notify();
        }));
        if view.conflicts.is_empty() {
            conflicts = conflicts.row(SettingsRow::new(w.no_conflicts));
        }
        for conflict in &view.conflicts {
            let mut row =
                SettingsRow::new(conflict.explanation(c)).description(conflict.resolution_label(c));
            if let Some(note) = conflict.overridden_note(c) {
                row = row.content(label(note, CAPTION, p.text_secondary));
            }
            conflicts = conflicts.row(row);
        }
        rows.push(Row::new(CARD_GAP, conflicts));

        // Quick sessions.
        rows.push(self.group_header(w.quick_group, cx));
        rows.push(Row::card(
            FIRST_CARD_GAP,
            card::QUICK,
            self.quick_expander(
                &offer,
                offer_document.as_ref().is_none_or(|d| d.writable),
                busy,
                cx,
            ),
        ));

        // Defaults for new sessions.
        rows.push(self.group_header(w.defaults_group, cx));
        let writable = offer_document
            .as_ref()
            .is_none_or(|document| document.writable);
        let policy = offer.defaults.policy;
        for (index, field) in DefaultField::SWITCHES.into_iter().enumerate() {
            let on = field.get(&policy);
            let id = match field {
                DefaultField::SystemSleep => card::DEFAULT_SLEEP,
                DefaultField::DisplaySleep => card::DEFAULT_DISPLAY,
                _ => "",
            };
            let mut switch_card = SettingsCard::new(
                ElementId::named_usize("awake-default", index),
                field.label(w, c),
            )
            .content(
                ToggleSwitch::new(ElementId::named_usize("awake-default-switch", index), on)
                    .locale(locale.words())
                    .disabled(busy || !writable)
                    .on_change(Self::act_with(cx, move |this, on: bool, _, cx| {
                        this.set_default_field(field, on, cx)
                    })),
            );
            if field.warns(on) {
                switch_card = switch_card
                    .status(Severity::Warning)
                    .description(c.reduced_security_warning);
            }
            let gap = if index == 0 { FIRST_CARD_GAP } else { CARD_GAP };
            rows.push(if id.is_empty() {
                Row::new(gap, switch_card)
            } else {
                Row::card(gap, id, switch_card)
            });
        }
        // Also when the lid is closed (ticket 109), with its safety beside it.
        let battery = view.battery;
        let has_battery = battery.offers_threshold();
        let lid_on = policy.prevent_lid_suspend;
        let mut lid_card =
            SettingsCard::new(card::DEFAULT_LID, c.keep_awake_lid_closed).icon(IconName::Moon);
        if view.lid.available {
            lid_card = lid_card
                .description(format!(
                    "{}\n{}",
                    super::model::lid_safety_note(
                        view.lid.limit,
                        offer.defaults.battery_stop_percent,
                        has_battery,
                        locale
                    ),
                    c.lid_external_display_note
                ))
                .content(
                    ToggleSwitch::new("awake-default-lid-switch", lid_on)
                        .locale(locale.words())
                        .disabled(busy || !writable)
                        .on_change(Self::act_with(cx, |this, on: bool, _, cx| {
                            this.set_default_field(DefaultField::Lid, on, cx)
                        })),
                );
        } else {
            lid_card = lid_card.description(c.lid_backend_cannot).disabled(true);
        }
        rows.push(Row::card(CARD_GAP, card::DEFAULT_LID, lid_card));
        // The battery stop: not drawn on a machine with no battery.
        if has_battery {
            let percent = offer
                .defaults
                .battery_stop_percent
                .unwrap_or(awake_core::DEFAULT_BATTERY_STOP_PERCENT);
            let stops = offer.defaults.battery_stop_percent.is_some();
            let input = self.battery_input(percent, window, cx);
            rows.push(Row::card(
                CARD_GAP,
                card::BATTERY_STOP,
                SettingsCard::new(card::BATTERY_STOP, w.battery_stop_header)
                    .icon(IconName::BatteryLow)
                    .content(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .child(
                                ToggleSwitch::new("awake-battery-switch", stops)
                                    .locale(locale.words())
                                    .disabled(busy || !writable)
                                    .on_change(Self::act_with(cx, move |this, on: bool, _, cx| {
                                        this.set_default_battery(on.then_some(percent), cx)
                                    })),
                            )
                            .child(
                                NumberBox::new(
                                    "awake-battery-percent",
                                    &input,
                                    f64::from(percent),
                                    NumberModel::new(
                                        f64::from(awake_ipc::MIN_BATTERY_STOP_PERCENT),
                                        f64::from(awake_ipc::MAX_BATTERY_STOP_PERCENT),
                                    ),
                                )
                                .width(px(110.0))
                                .disabled(!stops || busy || !writable)
                                .on_change(Self::act_with(
                                    cx,
                                    |this, value: f64, _, cx| {
                                        this.set_default_battery(Some(value.round() as u8), cx)
                                    },
                                )),
                            ),
                    ),
            ));
        }
        if !writable {
            rows.push(Row::new(
                CARD_GAP,
                label(w.offer_not_writable, CAPTION, p.text_secondary),
            ));
        }

        // Battery and safety.
        rows.push(self.group_header(w.battery_group, cx));
        let mut gap = FIRST_CARD_GAP;
        if let BatteryView::Present {
            percent,
            on_ac_power,
            ..
        } = battery
        {
            let source = match on_ac_power {
                Some(true) => c.on_ac_power,
                Some(false) => c.on_battery_power,
                None => c.unknown,
            };
            let charge = percent
                .map(|percent| {
                    fill(
                        &fill(w.charge_line, "percent", &percent.to_string()),
                        "source",
                        source,
                    )
                })
                .unwrap_or_else(|| source.to_string());
            rows.push(Row::new(
                gap,
                SettingsCard::new("awake-charge", c.current_charge)
                    .icon(IconName::BatteryMedium)
                    .description(c.on_battery_rule_detail)
                    .content(label(charge, BODY, p.text_secondary)),
            ));
            gap = CARD_GAP;
            rows.push(Row::new(
                gap,
                SettingsCard::new("awake-battery-rule", c.on_battery_rule).content(label(
                    battery.summary(c),
                    BODY,
                    p.text_secondary,
                )),
            ));
        }
        let limits: Vec<ComboItem> = LID_LIMITS
            .iter()
            .map(|minutes| {
                ComboItem::new(match minutes {
                    Some(minutes) => super::copy::limit_words(locale, *minutes),
                    None => c.lid_limit_never.to_string(),
                })
            })
            .collect();
        rows.push(Row::card(
            gap,
            card::LID_LIMIT,
            SettingsCard::new(card::LID_LIMIT, c.lid_limit_heading)
                .icon(IconName::Moon)
                .description(c.lid_limit_description)
                .disabled(!view.lid.available)
                .content(
                    ComboBox::new(
                        "awake-lid-limit-choice",
                        limits,
                        decide::lid_limit_index(view.lid.limit),
                    )
                    .min_width(px(CONTROL_WIDTH))
                    .disabled(busy || !view.lid.available)
                    .on_select(Self::act_with(
                        cx,
                        |this, index: usize, _, cx| {
                            if let Some(minutes) = LID_LIMITS.get(index) {
                                this.set_lid_limit(*minutes, cx)
                            }
                        },
                    )),
                ),
        ));
        rows.push(Row::new(
            12.0,
            InfoBar::new(
                "awake-quit-warning",
                Severity::Informational,
                c.quit_warning,
            )
            .message(c.quit_warning_detail)
            .locale(locale.words()),
        ));
        rows
    }

    fn headline_control(
        &self,
        headline: &Headline,
        offer: &QuickSessions,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let w = page_copy(self.locale);
        match headline {
            Headline::Nothing => {
                let mut entries: Vec<MenuEntry> = offer
                    .lengths
                    .iter()
                    .map(|length| {
                        let length = *length;
                        MenuItem::new(length_label(length, self.locale))
                            .on_select(Self::act(cx, move |this, _, cx| {
                                this.start_keep_awake_length(length, cx)
                            }))
                            .entry()
                    })
                    .collect();
                entries.push(MenuEntry::Separator);
                entries.push(
                    MenuItem::new(w.until_a_time)
                        .on_select(Self::act(cx, |this, _, cx| this.open_until_dialog(cx)))
                        .entry(),
                );
                let main = match offer.default_length() {
                    Some(length) => {
                        format!("{} · {}", w.keep_awake, length_label(length, self.locale))
                    }
                    None => w.keep_awake.to_string(),
                };
                SplitButton::new("awake-start", main)
                    .accent()
                    .entries(entries)
                    .disabled(busy)
                    .on_click(Self::act(cx, |this, _, cx| {
                        this.start_keep_awake_default(cx)
                    }))
                    .into_any_element()
            }
            Headline::Manual {
                session_id,
                remaining,
                ..
            } => {
                let session_id = *session_id;
                let timed = matches!(remaining, WireRemaining::Seconds { .. });
                let mut row = div().flex().items_center().gap(px(8.0));
                if timed {
                    let entries = [(15, w.pause_15), (30, "30"), (60, w.pause_60)]
                        .into_iter()
                        .map(|(minutes, _)| {
                            MenuItem::new(match minutes {
                                15 => w.extend_15,
                                30 => w.extend_30,
                                _ => w.extend_60,
                            })
                            .on_select(Self::act(cx, move |this, _, cx| {
                                this.extend_keep_awake(session_id, minutes, cx)
                            }))
                            .entry()
                        })
                        .collect();
                    row = row.child(
                        DropDownButton::new("awake-extend", ButtonKind::Standard)
                            .label(w.extend)
                            .entries(entries)
                            .disabled(busy),
                    );
                }
                row.child(
                    Button::standard("awake-end", w.end)
                        .disabled(busy)
                        .on_click(Self::act(cx, |this, _, cx| this.end_keep_awake_manual(cx))),
                )
                .into_any_element()
            }
            Headline::RulesOnly { .. } => DropDownButton::new("awake-pause", ButtonKind::Standard)
                .label(w.pause_rules)
                .entries(self.pause_entries(cx))
                .disabled(busy)
                .into_any_element(),
            Headline::Paused { .. } | Headline::Overridden => {
                Button::standard("awake-resume", w.resume_rules)
                    .disabled(busy)
                    .on_click(Self::act(cx, |this, _, cx| {
                        this.resume_keep_awake_rules(cx)
                    }))
                    .into_any_element()
            }
            Headline::Attention { .. } => {
                Button::standard("awake-see-diagnostics", w.see_diagnostics)
                    .on_click(Self::act(cx, |this, _, cx| {
                        this.set_keep_awake_sub(SubPage::Diagnostics, cx)
                    }))
                    .into_any_element()
            }
        }
    }

    fn pause_entries(&self, cx: &mut Context<Self>) -> Vec<MenuEntry> {
        let w = page_copy(self.locale);
        [
            (w.pause_15, Some(awake_core::PAUSE_SHORT_SECONDS)),
            (w.pause_60, Some(awake_core::PAUSE_LONG_SECONDS)),
            (w.pause_until_resumed, None),
        ]
        .into_iter()
        .map(|(text, seconds)| {
            MenuItem::new(text)
                .on_select(Self::act(cx, move |this, _, cx| {
                    this.pause_keep_awake_rules(seconds, cx)
                }))
                .entry()
        })
        .collect()
    }

    fn quick_expander(
        &self,
        offer: &QuickSessions,
        writable: bool,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> SettingsExpander {
        let w = page_copy(self.locale);
        let p = palette(cx);
        let disabled = busy || !writable;
        let addable = decide::addable_lengths(offer);
        let add_items: Vec<ComboItem> = addable
            .iter()
            .map(|length| ComboItem::new(length_label(*length, self.locale)))
            .collect();
        let add_box = ComboBox::new("awake-add-length", add_items, None)
            .placeholder(if addable.is_empty() {
                w.quick_full
            } else {
                w.add_length_placeholder
            })
            .min_width(px(CONTROL_WIDTH))
            .disabled(disabled || addable.is_empty())
            .on_select(Self::act_with(cx, move |this, index: usize, _, cx| {
                if let Some(length) = addable.get(index).copied() {
                    this.change_offer(|offer| offer.add_length(length), cx);
                }
            }));
        let mut expander =
            SettingsExpander::new(card::QUICK, w.quick_header, self.keep_awake.quick_open)
                .icon(IconName::Play)
                .description(w.quick_description)
                .on_toggle(Self::act_with(cx, |this, open: bool, _, cx| {
                    this.keep_awake.quick_open = open;
                    cx.notify();
                }));
        for (index, row) in decide::length_rows(offer).into_iter().enumerate() {
            let mut buttons = div().flex().items_center().gap(px(4.0));
            if row.is_default {
                buttons = buttons.child(div().px(px(8.0)).child(label(
                    w.is_default,
                    CAPTION,
                    p.text_secondary,
                )));
            } else {
                buttons = buttons.child(
                    Button::standard(
                        ElementId::named_usize("awake-make-default", index),
                        w.make_default,
                    )
                    .disabled(disabled)
                    .on_click(Self::act(cx, move |this, _, cx| {
                        this.change_offer(|offer| offer.set_default(index), cx)
                    })),
                );
            }
            buttons = buttons
                .child(
                    Button::icon_button(
                        ElementId::named_usize("awake-length-up", index),
                        IconName::ChevronUp,
                        w.move_up,
                    )
                    .disabled(disabled || !row.can_move_up)
                    .on_click(Self::act(cx, move |this, _, cx| {
                        this.change_offer(|offer| offer.move_length(index, -1), cx)
                    })),
                )
                .child(
                    Button::icon_button(
                        ElementId::named_usize("awake-length-down", index),
                        IconName::ChevronDown,
                        w.move_down,
                    )
                    .disabled(disabled || !row.can_move_down)
                    .on_click(Self::act(cx, move |this, _, cx| {
                        this.change_offer(|offer| offer.move_length(index, 1), cx)
                    })),
                )
                .child(
                    Button::icon_button(
                        ElementId::named_usize("awake-length-remove", index),
                        IconName::Delete,
                        w.remove,
                    )
                    .disabled(disabled || !row.can_remove)
                    .on_click(Self::act(cx, move |this, _, cx| {
                        this.change_offer(|offer| offer.remove_length(index), cx)
                    })),
                );
            expander = expander
                .row(SettingsRow::new(length_label(row.length, self.locale)).content(buttons));
        }
        expander
            .row(SettingsRow::new(w.add_length).content(add_box))
            .row(
                SettingsRow::new(w.restore_defaults).content(
                    Button::standard("awake-restore-lengths", w.restore_defaults)
                        .disabled(disabled)
                        .on_click(Self::act(cx, |this, _, cx| {
                            this.change_offer(
                                |offer| {
                                    let before = offer.clone();
                                    offer.restore_lengths();
                                    *offer != before
                                },
                                cx,
                            )
                        })),
                ),
            )
    }

    // ---- Rules ------------------------------------------------------------

    fn awake_rules_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Row> {
        let locale = self.locale;
        let c = copy(locale);
        let w = page_copy(locale);
        let p = palette(cx);
        let busy = self.keep_awake.busy;
        let Some(status) = self.keep_awake.status().cloned() else {
            return Vec::new();
        };
        let view = StatusView::from_status(&status);
        let mut rows = Vec::new();

        let mut bar = CommandBar::new("awake-rules-bar")
            .locale(locale.words())
            .command(
                Command::new(w.new_rule, IconName::Plus)
                    .disabled(busy)
                    .on_click(Self::act(cx, |this, window, cx| this.new_rule(window, cx))),
            )
            .command(
                Command::new(c.pause_rules_short, IconName::Pause)
                    .disabled(busy)
                    .on_click(Self::act(cx, |this, _, cx| {
                        this.pause_keep_awake_rules(Some(awake_core::PAUSE_SHORT_SECONDS), cx)
                    })),
            )
            .command(
                Command::new(c.pause_rules_long, IconName::Pause)
                    .disabled(busy)
                    .on_click(Self::act(cx, |this, _, cx| {
                        this.pause_keep_awake_rules(Some(awake_core::PAUSE_LONG_SECONDS), cx)
                    })),
            )
            .command(
                Command::new(w.override_all, IconName::CircleX)
                    .disabled(busy)
                    .on_click(Self::act(cx, |this, _, cx| {
                        this.keep_awake.dialog = Some(Dialog::OverrideAll);
                        cx.notify();
                    })),
            );
        if view.suppression.is_some() {
            bar = bar.command(
                Command::new(w.resume_rules, IconName::Play)
                    .disabled(busy)
                    .on_click(Self::act(cx, |this, _, cx| {
                        this.resume_keep_awake_rules(cx)
                    })),
            );
        }
        rows.push(Row::card(0.0, card::RULES, bar));
        if let Some(suppression) = view.suppression {
            rows.push(Row::new(
                CARD_GAP,
                InfoBar::new(
                    "awake-rules-suppressed",
                    Severity::Warning,
                    suppression_label(suppression, c),
                )
                .locale(locale.words()),
            ));
        }
        let summary = fill(
            &fill(w.rules_summary, "enabled", &view.rules_enabled.to_string()),
            "total",
            &view.rules_total.to_string(),
        );
        rows.push(Row::new(8.0, label(summary, CAPTION, p.text_secondary)));
        if view.rules_refused > 0 {
            rows.push(Row::new(
                CARD_GAP,
                label(
                    fill(c.refused_rules, "count", &view.rules_refused.to_string()),
                    CAPTION,
                    p.caution,
                ),
            ));
        }

        let rules = self.keep_awake.rule_views(locale);
        if rules.is_empty() {
            rows.push(Row::new(
                12.0,
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .child(label(w.rules_empty, BODY, p.text_secondary))
                    .child(
                        div().flex().child(
                            Button::standard("awake-rules-empty-new", w.new_rule).on_click(
                                Self::act(cx, |this, window, cx| this.new_rule(window, cx)),
                            ),
                        ),
                    ),
            ));
            return rows;
        }
        let count = rules.len();
        for (index, rule) in rules.into_iter().enumerate() {
            let rule_id = rule.rule_id;
            let open = self.keep_awake.open_rules.contains(&rule_id);
            let priority = self.priority_input(rule_id, rule.priority, window, cx);
            let name = rule.name.clone();
            let mut entries = vec![
                MenuItem::new(w.edit)
                    .on_select(Self::act(cx, move |this, window, cx| {
                        this.edit_rule(rule_id, window, cx)
                    }))
                    .entry(),
                MenuItem::new(w.duplicate)
                    .on_select(Self::act(cx, move |this, _, cx| {
                        this.duplicate_rule(rule_id, cx)
                    }))
                    .entry(),
                MenuItem::new(w.move_up)
                    .disabled(index == 0)
                    .on_select(Self::act(cx, move |this, _, cx| {
                        this.move_rule(rule_id, -1, cx)
                    }))
                    .entry(),
                MenuItem::new(w.move_down)
                    .disabled(index + 1 == count)
                    .on_select(Self::act(cx, move |this, _, cx| {
                        this.move_rule(rule_id, 1, cx)
                    }))
                    .entry(),
            ];
            entries.push(MenuEntry::Separator);
            entries.push(
                MenuItem::new(w.delete)
                    .on_select(Self::act(cx, move |this, _, cx| {
                        this.ask_delete_rule(rule_id, name.clone(), cx)
                    }))
                    .entry(),
            );
            let mut expander = SettingsExpander::new(
                ElementId::named_usize("awake-rule", index),
                rule.name.clone(),
                open,
            )
            .icon(IconName::Bot)
            .description(decide::rule_description(&rule, c, w))
            .content(
                ToggleSwitch::new(
                    ElementId::named_usize("awake-rule-switch", index),
                    rule.enabled,
                )
                .locale(locale.words())
                .disabled(busy)
                .on_change(Self::act_with(cx, move |this, on: bool, _, cx| {
                    this.set_rule_enabled(rule_id, on, cx)
                })),
            )
            .on_toggle(Self::act_with(cx, move |this, open: bool, _, cx| {
                if open {
                    this.keep_awake.open_rules.insert(rule_id);
                } else {
                    this.keep_awake.open_rules.remove(&rule_id);
                }
                cx.notify();
            }))
            .row(
                SettingsRow::new(w.priority)
                    .description(w.priority_description)
                    .content(
                        NumberBox::new(
                            ElementId::named_usize("awake-rule-priority", index),
                            &priority,
                            f64::from(rule.priority),
                            NumberModel::new(0.0, 255.0),
                        )
                        .width(px(120.0))
                        .disabled(busy)
                        .on_change(Self::act_with(
                            cx,
                            move |this, value: f64, _, cx| {
                                this.set_rule_priority(
                                    rule_id,
                                    value.round().clamp(0.0, 255.0) as u8,
                                    cx,
                                )
                            },
                        )),
                    ),
            )
            .row(SettingsRow::new(c.lid_rule_heading).content(label(
                if rule.policy.prevent_lid_suspend {
                    w.lid_rule_on
                } else {
                    w.lid_rule_off
                },
                BODY,
                p.text_secondary,
            )));
            for group in &rule.groups {
                for condition in &group.conditions {
                    if let Some(explanation) = condition.explanation() {
                        expander = expander.row(
                            SettingsRow::new(fill(
                                c.condition_unavailable,
                                "provider",
                                provider_label(condition.provider, c),
                            ))
                            .description(explanation.to_string()),
                        );
                    }
                }
            }
            let test = self
                .keep_awake
                .test
                .as_ref()
                .filter(|test| test.rule_id == rule_id)
                .map(|test| test_summary(test, c));
            let mut test_row = SettingsRow::new(c.test_rule);
            if let Some(summary) = test {
                test_row = test_row.description(summary);
            }
            expander = expander.row(
                test_row.content(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(
                            Button::standard(
                                ElementId::named_usize("awake-rule-test", index),
                                w.test,
                            )
                            .disabled(busy)
                            .on_click(Self::act(cx, move |this, _, cx| {
                                this.test_keep_awake_rule(rule_id, cx)
                            })),
                        )
                        .child(
                            DropDownButton::more(
                                ElementId::named_usize("awake-rule-more", index),
                                w.rule_actions,
                            )
                            .entries(entries)
                            .disabled(busy)
                            .align_right(),
                        ),
                ),
            );
            rows.push(Row::new(if index == 0 { 12.0 } else { CARD_GAP }, expander));
        }
        rows
    }

    // ---- The rule editor --------------------------------------------------

    fn rule_editor_rows(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Vec<Row> {
        let locale = self.locale;
        let c = copy(locale);
        let w = page_copy(locale);
        let p = palette(cx);
        let providers = self.keep_awake.providers();
        let Some(draft) = self.keep_awake.draft.as_ref() else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        if let Some(error) = &draft.error {
            rows.push(Row::new(
                0.0,
                div().pb(px(12.0)).child(
                    InfoBar::new("awake-draft-error", Severity::Error, c.rule_invalid_input)
                        .message(error.clone())
                        .locale(locale.words()),
                ),
            ));
        }
        let combine_items =
            |id: ElementId, combine: Combine, on_pick: PickCombine, cx: &mut Context<Self>| {
                let on_pick = std::rc::Rc::new(on_pick);
                ComboBox::new(
                    id,
                    vec![ComboItem::new(c.match_all), ComboItem::new(c.match_any)],
                    Some(match combine {
                        Combine::All => 0,
                        Combine::Any => 1,
                    }),
                )
                .min_width(px(CONTROL_WIDTH))
                .on_select(Self::act_with(cx, move |this, index: usize, _, cx| {
                    on_pick(
                        if index == 0 {
                            Combine::All
                        } else {
                            Combine::Any
                        },
                        this,
                        cx,
                    )
                }))
            };

        rows.push(Row::new(
            0.0,
            SettingsCard::new("awake-draft-name", c.rule_name)
                .content(TextBox::new("awake-draft-name-box", &draft.name).width(px(280.0))),
        ));
        rows.push(Row::new(
            CARD_GAP,
            SettingsCard::new("awake-draft-enabled", w.rule_enabled).content(
                ToggleSwitch::new("awake-draft-enabled-switch", draft.enabled)
                    .locale(locale.words())
                    .on_change(Self::act_with(cx, |this, on: bool, _, cx| {
                        this.update_draft(|draft| draft.enabled = on, cx)
                    })),
            ),
        ));
        rows.push(Row::new(
            CARD_GAP,
            SettingsCard::new("awake-draft-priority", w.priority)
                .description(w.priority_description)
                .content(
                    NumberBox::new(
                        "awake-draft-priority-box",
                        &draft.priority_input,
                        f64::from(draft.priority),
                        NumberModel::new(0.0, 255.0),
                    )
                    .width(px(120.0))
                    .on_change(Self::act_with(
                        cx,
                        |this, value: f64, _, cx| {
                            this.update_draft(
                                |draft| draft.priority = value.round().clamp(0.0, 255.0) as u8,
                                cx,
                            )
                        },
                    )),
                ),
        ));
        rows.push(Row::new(
            CARD_GAP,
            SettingsCard::new("awake-draft-combine", w.group_combine).content(combine_items(
                "awake-draft-combine-choice".into(),
                draft.combine,
                Box::new(|combine, this, cx| {
                    this.update_draft(|draft| draft.combine = combine, cx)
                }),
                cx,
            )),
        ));
        let lid_on = draft.policy.prevent_lid_suspend;
        rows.push(Row::new(
            CARD_GAP,
            SettingsCard::new("awake-draft-lid", c.keep_awake_lid_closed)
                .icon(IconName::Moon)
                .description(format!("{} · {}", w.when_this_rule, c.lid_rule_default_off))
                .content(
                    ToggleSwitch::new("awake-draft-lid-switch", lid_on)
                        .locale(locale.words())
                        .on_change(Self::act_with(cx, |this, on: bool, _, cx| {
                            this.update_draft(|draft| draft.policy.prevent_lid_suspend = on, cx)
                        })),
                ),
        ));

        let group_count = draft.groups.len();
        for (group_index, group) in draft.groups.iter().enumerate() {
            rows.push(Row::new(
                GROUP_GAP,
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label(
                        fill(w.group_title, "n", &(group_index + 1).to_string()),
                        better_ui::fluent::BODY_STRONG,
                        p.text_primary,
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(combine_items(
                                ElementId::named_usize("awake-group-combine", group_index),
                                group.combine,
                                Box::new(move |combine, this, cx| {
                                    this.update_draft(
                                        |draft| {
                                            if let Some(group) = draft.groups.get_mut(group_index) {
                                                group.combine = combine;
                                            }
                                        },
                                        cx,
                                    )
                                }),
                                cx,
                            ))
                            .when(group_count > 1, |row| {
                                row.child(
                                    Button::icon_button(
                                        ElementId::named_usize("awake-group-remove", group_index),
                                        IconName::Delete,
                                        w.remove_group,
                                    )
                                    .on_click(Self::act(cx, move |this, _, cx| {
                                        this.remove_group(group_index, cx)
                                    })),
                                )
                            }),
                    ),
            ));
            let condition_count = group.conditions.len();
            for (index, condition) in group.conditions.iter().enumerate() {
                let view = ConditionView::present(&condition.condition, &providers, c);
                let id = group_index * 100 + index;
                let mut condition_card = SettingsCard::new(
                    ElementId::named_usize("awake-condition", id),
                    provider_label(view.provider, c),
                );
                if let Some(explanation) = view.explanation() {
                    condition_card = condition_card
                        .description(fill(c.condition_unavailable_detail, "reason", explanation))
                        .disabled(true);
                } else {
                    condition_card = condition_card
                        .description(view.summary.clone())
                        .content(self.operand_controls(group_index, index, condition, cx));
                }
                let remove = Button::icon_button(
                    ElementId::named_usize("awake-condition-remove", id),
                    IconName::Close,
                    w.remove_condition,
                )
                .disabled(condition_count <= 1)
                .on_click(Self::act(cx, move |this, _, cx| {
                    this.remove_condition(group_index, index, cx)
                }));
                rows.push(Row::new(
                    if index == 0 { FIRST_CARD_GAP } else { CARD_GAP },
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(div().flex_1().min_w_0().child(condition_card))
                        .child(remove),
                ));
            }
            let entries = ProviderKind::ALL
                .iter()
                .map(|provider| {
                    let provider = *provider;
                    let row = providers.iter().find(|row| row.kind == provider);
                    let unavailable = row.is_some_and(|row| !row.available);
                    MenuItem::new(provider_label(provider, c))
                        .disabled(unavailable)
                        .on_select(Self::act(cx, move |this, window, cx| {
                            this.add_condition(group_index, provider, window, cx)
                        }))
                        .entry()
                })
                .collect();
            rows.push(Row::new(
                CARD_GAP,
                div().flex().child(
                    DropDownButton::new(
                        ElementId::named_usize("awake-add-condition", group_index),
                        ButtonKind::Standard,
                    )
                    .icon(IconName::Plus)
                    .label(w.add_condition)
                    .entries(entries)
                    .disabled(condition_count >= awake_core::MAX_CONDITIONS_PER_GROUP),
                ),
            ));
        }
        rows.push(Row::new(
            12.0,
            div().flex().child(
                Button::standard("awake-add-group", w.add_group)
                    .icon(IconName::Plus)
                    .disabled(group_count >= awake_core::MAX_GROUPS_PER_RULE)
                    .on_click(Self::act(cx, |this, window, cx| this.add_group(window, cx))),
            ),
        ));
        rows.push(Row::new(
            GROUP_GAP,
            div()
                .flex()
                .gap(px(8.0))
                .child(
                    Button::accent("awake-draft-save", w.save)
                        .disabled(self.keep_awake.busy)
                        .on_click(Self::act(cx, |this, _, cx| this.save_draft(cx))),
                )
                .child(
                    Button::standard("awake-draft-cancel", w.cancel)
                        .on_click(Self::act(cx, |this, _, cx| this.close_draft(cx))),
                ),
        ));
        rows
    }

    /// The controls one condition's operands are edited with.
    fn operand_controls(
        &self,
        group: usize,
        index: usize,
        draft: &super::state::DraftCondition,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let locale = self.locale;
        let c = copy(locale);
        let w = page_copy(locale);
        let id = group * 100 + index;
        let secondary = palette(cx).text_secondary;
        let choice = |labels: [&'static str; 2],
                      on: bool,
                      set: fn(&mut Condition, bool),
                      cx: &mut Context<Self>| {
            ComboBox::new(
                ElementId::named_usize("awake-operand-choice", id),
                vec![ComboItem::new(labels[0]), ComboItem::new(labels[1])],
                Some(if on { 0 } else { 1 }),
            )
            .min_width(px(CONTROL_WIDTH))
            .on_select(Self::act_with(cx, move |this, picked: usize, _, cx| {
                this.update_condition(group, index, |condition| set(condition, picked == 0), cx)
            }))
            .into_any_element()
        };
        let number = |slot: usize,
                      value: f64,
                      minimum: f64,
                      maximum: f64,
                      unit: Option<&'static str>|
         -> AnyElement {
            let Some(input) = draft.numbers.get(slot) else {
                return div().into_any_element();
            };
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    NumberBox::new(
                        ElementId::named_usize("awake-operand-number", id * 10 + slot),
                        input,
                        value,
                        NumberModel::new(minimum, maximum),
                    )
                    .width(px(110.0)),
                )
                .children(unit.map(|unit| label(unit, BODY, secondary)))
                .into_any_element()
        };
        match &draft.condition {
            Condition::AcPower { connected } => choice(
                [c.condition_ac_connected, c.condition_ac_disconnected],
                *connected,
                |condition, on| *condition = Condition::AcPower { connected: on },
                cx,
            ),
            Condition::ExternalDisplay { connected } => choice(
                [
                    c.condition_external_display_connected,
                    c.condition_external_display_disconnected,
                ],
                *connected,
                |condition, on| *condition = Condition::ExternalDisplay { connected: on },
                cx,
            ),
            Condition::AudioPlayback { playing } => choice(
                [c.condition_audio_playing, c.condition_audio_silent],
                *playing,
                |condition, on| *condition = Condition::AudioPlayback { playing: on },
                cx,
            ),
            Condition::Fullscreen { active } => choice(
                [
                    c.condition_fullscreen_active,
                    c.condition_fullscreen_inactive,
                ],
                *active,
                |condition, on| *condition = Condition::Fullscreen { active: on },
                cx,
            ),
            Condition::ProcessRunning { matcher } => {
                let kind = matcher.kind;
                let mut row = div().flex().items_center().gap(px(8.0)).child(
                    ComboBox::new(
                        ElementId::named_usize("awake-operand-matcher", id),
                        vec![
                            ComboItem::new(c.matcher_executable),
                            ComboItem::new(c.matcher_desktop_id),
                        ],
                        Some(match kind {
                            ProcessMatchKind::ExecutableName => 0,
                            ProcessMatchKind::DesktopId => 1,
                        }),
                    )
                    .min_width(px(160.0))
                    .on_select(Self::act_with(
                        cx,
                        move |this, picked: usize, _, cx| {
                            this.update_condition(
                                group,
                                index,
                                |condition| {
                                    if let Condition::ProcessRunning { matcher } = condition {
                                        let kind = if picked == 0 {
                                            ProcessMatchKind::ExecutableName
                                        } else {
                                            ProcessMatchKind::DesktopId
                                        };
                                        if let Ok(next) =
                                            awake_core::ProcessMatcher::new(kind, matcher.as_str())
                                        {
                                            *matcher = next;
                                        }
                                    }
                                },
                                cx,
                            )
                        },
                    )),
                );
                if let Some(text) = &draft.text {
                    row = row.child(
                        TextBox::new(ElementId::named_usize("awake-operand-text", id), text)
                            .width(px(200.0)),
                    );
                }
                row.into_any_element()
            }
            Condition::NetworkInterfaceUp { .. } => match &draft.text {
                Some(text) => TextBox::new(ElementId::named_usize("awake-operand-text", id), text)
                    .width(px(160.0))
                    .into_any_element(),
                None => div().into_any_element(),
            },
            Condition::WatchedPathActive { within_seconds, .. } => {
                let mut row = div().flex().items_center().gap(px(8.0));
                if let Some(text) = &draft.text {
                    row = row.child(
                        TextBox::new(ElementId::named_usize("awake-operand-text", id), text)
                            .width(px(220.0)),
                    );
                }
                row.child(label(c.watch_window, BODY, palette(cx).text_secondary))
                    .child(number(
                        0,
                        *within_seconds as f64,
                        1.0,
                        awake_core::MAX_WATCH_WINDOW_SECONDS as f64,
                        Some(c.second_unit),
                    ))
                    .into_any_element()
            }
            Condition::BatteryPercent { at_least, at_most } => div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(label(w.at_least, BODY, palette(cx).text_secondary))
                .child(number(0, f64::from(*at_least), 0.0, 100.0, Some("%")))
                .child(label(w.at_most, BODY, palette(cx).text_secondary))
                .child(number(1, f64::from(*at_most), 0.0, 100.0, Some("%")))
                .into_any_element(),
            Condition::CpuUtilizationAtLeast { percent } => {
                number(0, f64::from(*percent), 1.0, 100.0, Some("%"))
            }
            Condition::NetworkThroughputAtLeast {
                kibibytes_per_second,
            } => number(
                0,
                *kibibytes_per_second as f64,
                1.0,
                10_000_000.0,
                Some("KiB/s"),
            ),
            Condition::TimeSchedule { schedule } => {
                let schedule = schedule.clone();
                let days = Weekday::ALL.iter().enumerate().fold(
                    div().flex().gap(px(4.0)),
                    |row, (day_index, day)| {
                        let day = *day;
                        let on = schedule.days.contains(&day);
                        row.child(
                            Button::toggle(
                                ElementId::named_usize("awake-day", id * 10 + day_index),
                                weekday_label(day, c),
                                on,
                            )
                            .on_click(Self::act(
                                cx,
                                move |this, _, cx| {
                                    this.update_condition(
                                        group,
                                        index,
                                        |condition| {
                                            if let Condition::TimeSchedule { schedule } = condition
                                            {
                                                let mut days = schedule.days.clone();
                                                if on {
                                                    days.retain(|d| *d != day);
                                                } else {
                                                    days.push(day);
                                                }
                                                if let Ok(next) = awake_core::Schedule::new(
                                                    days,
                                                    schedule.from_minute_of_day,
                                                    schedule.to_minute_of_day,
                                                ) {
                                                    *schedule = next;
                                                }
                                            }
                                        },
                                        cx,
                                    )
                                },
                            )),
                        )
                    },
                );
                let time = |minute: u16| Time::from_minutes(u32::from(minute));
                let picker =
                    |slot: &'static str, minute: u16, from: bool, cx: &mut Context<Self>| {
                        TimePicker::new(ElementId::named_usize(slot, id), time(minute))
                            .model(TimeModel::new(5))
                            .width(px(140.0))
                            .on_change(Self::act_with(cx, move |this, picked: Time, _, cx| {
                                this.update_condition(
                                    group,
                                    index,
                                    |condition| {
                                        if let Condition::TimeSchedule { schedule } = condition {
                                            let minutes = picked.minutes() as u16;
                                            let (from_minute, to_minute) = if from {
                                                (minutes, schedule.to_minute_of_day)
                                            } else {
                                                (schedule.from_minute_of_day, minutes)
                                            };
                                            if let Ok(next) = awake_core::Schedule::new(
                                                schedule.days.clone(),
                                                from_minute,
                                                to_minute,
                                            ) {
                                                *schedule = next;
                                            }
                                        }
                                    },
                                    cx,
                                )
                            }))
                    };
                div()
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(label(c.schedule_days, BODY, secondary))
                            .child(days),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(label(c.schedule_from, BODY, palette(cx).text_secondary))
                            .child(picker(
                                "awake-schedule-from",
                                schedule.from_minute_of_day,
                                true,
                                cx,
                            ))
                            .child(label(c.schedule_to, BODY, palette(cx).text_secondary))
                            .child(picker(
                                "awake-schedule-to",
                                schedule.to_minute_of_day,
                                false,
                                cx,
                            )),
                    )
                    .into_any_element()
            }
        }
    }

    // ---- History -----------------------------------------------------------

    fn awake_history_rows(&mut self, cx: &mut Context<Self>) -> Vec<Row> {
        let locale = self.locale;
        let c = copy(locale);
        let w = page_copy(locale);
        let p = palette(cx);
        let offset = self.keep_awake.offset();
        let history = self.keep_awake.history();
        let cells = decide::history_cells(&history, c, offset);
        let columns = vec![
            Column::new(w.column_started)
                .width(px(150.0))
                .sortable(true),
            Column::new(w.column_ended).width(px(150.0)).sortable(true),
            Column::new(w.column_origin).width(px(90.0)).sortable(false),
            Column::new(w.column_reason).sortable(false),
            Column::new(w.column_stopped)
                .width(px(180.0))
                .sortable(false),
        ];
        let rows_cells: Vec<Vec<TableCell>> = cells
            .into_iter()
            .map(|row| row.into_iter().map(TableCell::from).collect())
            .collect();
        let mut rows = vec![Row::card(
            0.0,
            card::HISTORY,
            DetailsTable::new("awake-history-table", columns, rows_cells)
                .sort(self.keep_awake.history_sort)
                .empty(w.history_empty)
                .on_sort(Self::act_with(cx, |this, sort: SortState, _, cx| {
                    if decide::HISTORY_SORTABLE.contains(&sort.column) {
                        this.keep_awake.history_sort = Some(sort);
                        cx.notify();
                    }
                })),
        )];
        if let Some(document) = self
            .keep_awake
            .read
            .as_ref()
            .and_then(|read| read.history.as_ref())
        {
            let note = format!(
                "{} {}",
                fill(
                    c.retention_note,
                    "limit",
                    &document.retention_limit.to_string()
                ),
                fill(
                    &fill(c.showing_of, "shown", &document.entries.len().to_string()),
                    "total",
                    &document.total.to_string()
                ),
            );
            rows.push(Row::new(8.0, label(note, CAPTION, p.text_secondary)));
        }
        rows
    }

    // ---- Diagnostics ---------------------------------------------------------

    fn awake_diagnostics_rows(&mut self, cx: &mut Context<Self>) -> Vec<Row> {
        let locale = self.locale;
        let c = copy(locale);
        let w = page_copy(locale);
        let p = palette(cx);
        let Some(status) = self.keep_awake.status().cloned() else {
            return Vec::new();
        };
        let view = StatusView::from_status(&status);
        let backend = &view.backend;
        let mut rows = Vec::new();
        let mut backend_card = SettingsCard::new(card::DIAGNOSTICS, c.backend_heading)
            .icon(IconName::Cpu)
            .content(label(
                format!("{} · {}", backend.name, backend.availability_label(c)),
                BODY,
                if backend.available {
                    p.text_secondary
                } else {
                    p.caution
                },
            ));
        if let Some(detail) = &backend.detail {
            backend_card = backend_card.description(detail.clone());
        }
        if !backend.available {
            backend_card = backend_card.status(Severity::Warning);
        }
        rows.push(Row::card(0.0, card::DIAGNOSTICS, backend_card));
        rows.push(self.group_header(c.capability_heading, cx));
        for (index, row) in view.policy.iter().enumerate() {
            rows.push(Row::new(
                if index == 0 { FIRST_CARD_GAP } else { CARD_GAP },
                SettingsCard::new(
                    ElementId::named_usize("awake-capability", index),
                    row.field.label(c),
                )
                .content(label(
                    if row.delivered {
                        c.available
                    } else {
                        c.unavailable
                    },
                    BODY,
                    p.text_secondary,
                )),
            ));
        }
        rows.push(self.group_header(c.provider_heading, cx));
        let providers: Vec<ProviderRow> = status
            .providers
            .iter()
            .map(ProviderRow::from_wire)
            .collect();
        let cells: Vec<Vec<TableCell>> = providers
            .iter()
            .map(|row| {
                vec![
                    TableCell::from(provider_label(row.kind, c)),
                    TableCell::from(row.availability_label(c)),
                    TableCell::from(row.cadence_label(c)),
                    TableCell::from(row.explanation.clone().unwrap_or_default()),
                ]
            })
            .collect();
        rows.push(Row::new(
            FIRST_CARD_GAP,
            DetailsTable::new(
                "awake-providers",
                vec![
                    Column::new(w.column_provider).width(px(180.0)),
                    Column::new(w.column_availability).width(px(110.0)),
                    Column::new(w.column_polling).width(px(130.0)),
                    Column::new(w.column_reason_short),
                ],
                cells,
            ),
        ));
        rows.push(self.group_header(c.verification_heading, cx));
        rows.push(Row::new(
            FIRST_CARD_GAP,
            SettingsCard::new("awake-verified-inhibitor", c.verified_holds_no_inhibitor)
                .status(Severity::Success),
        ));
        rows.push(Row::new(
            CARD_GAP,
            SettingsCard::new("awake-verified-shell", c.verified_no_shell_command)
                .status(Severity::Success),
        ));
        rows.push(Row::new(
            CARD_GAP,
            SettingsCard::new("awake-tray-host", w.tray_host)
                .description(w.tray_host_described)
                .content(label(
                    c.tray_host_unavailable_detail,
                    CAPTION,
                    p.text_secondary,
                )),
        ));
        let version = self
            .keep_awake
            .read
            .as_ref()
            .and_then(|read| read.version)
            .map(|version| version.to_string())
            .unwrap_or_else(|| c.unknown.to_string());
        rows.push(Row::new(
            CARD_GAP,
            SettingsCard::new("awake-protocol", c.protocol_version).content(label(
                version,
                BODY,
                p.text_secondary,
            )),
        ));
        rows
    }

    // ---- Dialogs --------------------------------------------------------------

    /// The dialog open over the window, if any. Drawn as the root's last
    /// child.
    pub(crate) fn keep_awake_dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !matches!(self.page, crate::model::Page::KeepAwake) {
            return None;
        }
        let dialog = self.keep_awake.dialog.clone()?;
        let locale = self.locale;
        let c = copy(locale);
        let w = page_copy(locale);
        let p = palette(cx);
        let now = self
            .keep_awake
            .status()
            .map(|status| status.now_unix_seconds)
            .unwrap_or_else(manager_core::catalog::now_unix_seconds);
        let offset = super::localtime::UtcOffset::for_system(now);
        let buttons = |primary: &'static str| DialogButtons {
            primary: Some(primary.into()),
            secondary: None,
            close: Some(w.cancel.into()),
            default: DefaultButton::Primary,
        };
        Some(match dialog {
            Dialog::Until(time) => {
                let until = decide::until(now, offset, time);
                ContentDialog::new("awake-until-dialog", w.until_title)
                    .buttons(buttons(w.start))
                    .on_result(Self::act_with(
                        cx,
                        move |this, result: DialogResult, _, cx| match result {
                            DialogResult::Primary => this.start_keep_awake_until(time, cx),
                            _ => this.close_keep_awake_dialog(cx),
                        },
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .child(TimePicker::new("awake-until-time", time).on_change(
                                Self::act_with(cx, |this, picked: Time, _, cx| {
                                    this.keep_awake.dialog = Some(Dialog::Until(picked));
                                    cx.notify();
                                }),
                            ))
                            .child(label(
                                decide::until_caption(until, time, w),
                                CAPTION,
                                p.text_secondary,
                            )),
                    )
                    .into_any_element()
            }
            Dialog::Modify(draft) => {
                let offer = self
                    .keep_awake
                    .offer()
                    .map(|document| document.quick_sessions.clone())
                    .unwrap_or_default();
                let mut items: Vec<ComboItem> = offer
                    .lengths
                    .iter()
                    .map(|length| ComboItem::new(length_label(*length, self.locale)))
                    .collect();
                items.push(ComboItem::new(w.modify_until_option));
                let selected = draft.length.unwrap_or(offer.lengths.len());
                let lengths = offer.lengths.len();
                let mut content = div().flex().flex_col().gap(px(12.0)).child(
                    ComboBox::new("awake-modify-length", items, Some(selected))
                        .header(w.modify_length)
                        .min_width(px(240.0))
                        .on_select(Self::act_with(cx, move |this, index: usize, _, cx| {
                            this.update_modify(
                                |draft| draft.length = (index < lengths).then_some(index),
                                cx,
                            )
                        })),
                );
                if draft.length.is_none() {
                    let until = decide::until(now, offset, draft.until);
                    content = content
                        .child(
                            TimePicker::new("awake-modify-until", draft.until).on_change(
                                Self::act_with(cx, |this, picked: Time, _, cx| {
                                    this.update_modify(|draft| draft.until = picked, cx)
                                }),
                            ),
                        )
                        .child(label(
                            decide::until_caption(until, draft.until, w),
                            CAPTION,
                            p.text_secondary,
                        ));
                }
                content = content
                    .child(
                        ToggleSwitch::new(
                            "awake-modify-display",
                            draft.policy.prevent_display_sleep,
                        )
                        .header(w.prevent_display_sleep)
                        .locale(locale.words())
                        .on_change(Self::act_with(
                            cx,
                            |this, on: bool, _, cx| {
                                this.update_modify(
                                    |draft| draft.policy.prevent_display_sleep = on,
                                    cx,
                                )
                            },
                        )),
                    )
                    .child(
                        ToggleSwitch::new("awake-modify-lid", draft.policy.prevent_lid_suspend)
                            .header(c.keep_awake_lid_closed)
                            .locale(locale.words())
                            .on_change(Self::act_with(cx, |this, on: bool, _, cx| {
                                this.update_modify(
                                    |draft| draft.policy.prevent_lid_suspend = on,
                                    cx,
                                )
                            })),
                    );
                if draft.policy.prevent_display_sleep {
                    content = content.child(label(c.reduced_security_warning, CAPTION, p.caution));
                }
                ContentDialog::new("awake-modify-dialog", w.modify_title)
                    .buttons(buttons(w.save))
                    .on_result(Self::act_with(
                        cx,
                        |this, result: DialogResult, _, cx| match result {
                            DialogResult::Primary => this.save_modify(cx),
                            _ => this.close_keep_awake_dialog(cx),
                        },
                    ))
                    .child(content)
                    .into_any_element()
            }
            Dialog::DeleteRule { rule_id, name } => {
                ContentDialog::new("awake-delete-dialog", fill(w.delete_title, "rule", &name))
                    .buttons(buttons(w.delete))
                    .on_result(Self::act_with(
                        cx,
                        move |this, result: DialogResult, _, cx| match result {
                            DialogResult::Primary => this.delete_rule(rule_id, cx),
                            _ => this.close_keep_awake_dialog(cx),
                        },
                    ))
                    .child(label(w.delete_body, BODY, p.text_primary))
                    .into_any_element()
            }
            Dialog::OverrideAll => ContentDialog::new("awake-override-dialog", w.override_title)
                .buttons(buttons(w.override_all))
                .on_result(Self::act_with(
                    cx,
                    |this, result: DialogResult, _, cx| match result {
                        DialogResult::Primary => this.override_keep_awake_rules(cx),
                        _ => this.close_keep_awake_dialog(cx),
                    },
                ))
                .child(label(w.override_body, BODY, p.text_primary))
                .into_any_element(),
        })
    }

    /// The Home quick toggle (spec section 3): on starts the default length,
    /// off ends the person's own session.
    pub(crate) fn keep_awake_quick_card(&self, cx: &mut Context<Self>) -> Option<SettingsCard> {
        let w = page_copy(self.locale);
        let super::decide::Facts::Running { active, manual, .. } = self.keep_awake_facts() else {
            return None;
        };
        Some(
            SettingsCard::new("home-keep-awake", w.quick_toggle_header)
                .icon(IconName::Sun)
                .description(match (&active, manual) {
                    (Some(active), true) => active.clone(),
                    _ => w.quick_toggle_off.to_string(),
                })
                .content(
                    ToggleSwitch::new("home-keep-awake-switch", manual)
                        .locale(self.locale.words())
                        .disabled(self.keep_awake.busy)
                        .on_change(Self::act_with(cx, |this, on: bool, _, cx| {
                            this.toggle_keep_awake(on, cx)
                        })),
                ),
        )
    }
}

fn weekday_label(day: Weekday, c: &'static Copy) -> &'static str {
    match day {
        Weekday::Monday => c.weekday_monday,
        Weekday::Tuesday => c.weekday_tuesday,
        Weekday::Wednesday => c.weekday_wednesday,
        Weekday::Thursday => c.weekday_thursday,
        Weekday::Friday => c.weekday_friday,
        Weekday::Saturday => c.weekday_saturday,
        Weekday::Sunday => c.weekday_sunday,
    }
}

/// A rule test's answer in one line.
fn test_summary(test: &RuleTestView, c: &'static Copy) -> String {
    let mut parts = vec![format!(
        "{} · {}",
        RuleTestView::truth_label(test.truth, c),
        test.outcome_label(c)
    )];
    if test.rule_disabled {
        parts.push(c.tested_rule_is_disabled.to_string());
    }
    if let Some(suppression) = test.suppression {
        parts.push(suppression_label(suppression, c).to_string());
    }
    for provider in &test.unavailable {
        parts.push(format!(
            "{}: {}",
            provider_label(provider.kind, c),
            provider.explanation.clone().unwrap_or_default()
        ));
    }
    parts.join(" · ")
}

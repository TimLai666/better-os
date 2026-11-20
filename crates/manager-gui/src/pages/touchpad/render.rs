//! Draws the Touchpad page with the Fluent kit only. Every decision it draws
//! was made in `touchpad-views` or in [`super::view`].
//!
//! Two rules from the older window hold here:
//!
//! - A control its backend cannot read, apply, and verify is drawn disabled
//!   with its reason as the card's description, never as a working switch.
//!   One function decides it ([`TouchpadPage::setting_card`]).
//! - What you chose and what is in effect are both shown, because they can
//!   differ and the difference is the point.

use better_ui::fluent::button::Button;
use better_ui::fluent::chip::Chip;
use better_ui::fluent::info_bar::{ErrorBar, ErrorReport, InfoBar, Severity};
use better_ui::fluent::list::{Column, DetailsTable, TableCell};
use better_ui::fluent::selection::{ComboBox, ComboItem, Slider, SliderModel};
use better_ui::fluent::settings_card::{SettingsCard, SettingsExpander, SettingsRow};
use better_ui::fluent::staged_bar::StagedChangesBar;
use better_ui::fluent::tabs::{SelectorBar, SelectorItem};
use better_ui::fluent::toggle::ToggleSwitch;
use better_ui::fluent::{BODY, BODY_STRONG, CAPTION, TITLE, label, palette, radius};
use better_ui::icon_set::Picture;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use touchpad_core::{
    AccelerationProfile, ClickMethod, HealthState, RestoreScope, RunState, Section, SettingId,
    SettingValue,
};
use touchpad_platform::DeviceState;
use touchpad_views::i18n::{Copy as TpCopy, copy as tp_copy};
use touchpad_views::model::{Control, SettingRow};

use super::TouchpadPage;
use super::copy::page_copy;
use super::view::{self, Availability, Notice, SubPage, card};
use crate::i18n::shell;
use crate::pages::states;
use crate::render::{CARD_GAP, FIRST_CARD_GAP, GROUP_GAP, Row};

/// The width a choice list takes, so the cards' controls line up: enough
/// for "Depends on how many fingers press" / "依按下的手指數".
pub(super) const CHOICE_WIDTH: f32 = 240.0;
/// A slider's width beside its value.
const SLIDER_WIDTH: f32 = 220.0;

impl TouchpadPage {
    /// A kit handler that runs `f` on this page.
    pub(super) fn act(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> impl Fn(&mut Window, &mut App) + 'static {
        let weak = cx.entity().downgrade();
        move |_, cx| {
            let _ = weak.update(cx, |this, cx| f(this, cx));
        }
    }

    /// A kit handler told a value.
    pub(super) fn act_with<T: 'static>(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, T, &mut Context<Self>) + 'static,
    ) -> impl Fn(T, &mut Window, &mut App) + 'static {
        let weak = cx.entity().downgrade();
        move |value, _, cx| {
            let _ = weak.update(cx, |this, cx| f(this, value, cx));
        }
    }

    pub(super) fn c(&self) -> &'static TpCopy {
        tp_copy(view::views_locale(self.locale))
    }

    pub(super) fn group(&self, title: &'static str, cx: &App) -> Row {
        Row::new(
            GROUP_GAP,
            label(title, BODY_STRONG, palette(cx).text_primary),
        )
    }

    /// The whole page, as rows of the control center's content column.
    pub(crate) fn rows(&mut self, installed: Option<&str>, cx: &mut Context<Self>) -> Vec<Row> {
        let locale = self.locale;
        let words = locale.words();
        let c = self.c();
        let pc = page_copy(locale);
        let p = palette(cx);
        let mut rows = vec![Row::new(
            0.0,
            label(shell(locale).touchpad, TITLE, p.text_primary).pb(px(12.0)),
        )];

        // Not installed, or too old: one card with the one thing to do.
        if let Availability::Blocked(state) = view::availability(installed) {
            let described = states::describe(locale, &state);
            let mut card =
                SettingsCard::new("touchpad-component", described.text).icon(Picture::Touchpad);
            if let Some(action) = described.action {
                card = card.content(
                    Button::accent("touchpad-component-action", action)
                        .on_click(Self::act(cx, |this, cx| this.open_component(cx))),
                );
            }
            rows.push(Row::new(0.0, card));
            return rows;
        }

        for notice in view::notices(self.model.session(), self.model.safe_mode()) {
            let bar = match notice {
                Notice::NotGnome => {
                    InfoBar::new("touchpad-not-gnome", Severity::Informational, pc.not_gnome)
                        .locale(words)
                }
                Notice::SafeMode => {
                    InfoBar::new("touchpad-safe-mode", Severity::Warning, c.safe_mode_banner)
                        .locale(words)
                        .action(
                            Button::standard("touchpad-safe-mode-off", c.safe_mode_off)
                                .on_click(Self::act(cx, |this, cx| this.set_safe_mode(false, cx))),
                        )
                }
            };
            rows.push(Row::new(0.0, div().pb(px(12.0)).child(bar)));
        }

        if let Some(problem) = self.model.health().checks.iter().find(|check| {
            check.id == "touchpad.configuration" && check.state == HealthState::Failed
        }) {
            rows.push(Row::new(
                0.0,
                div().pb(px(12.0)).child(
                    ErrorBar::new(
                        "touchpad-configuration",
                        ErrorReport::new(
                            touchpad_views::i18n::problem_sentence("", view::views_locale(locale)),
                            problem.detail.clone(),
                        ),
                    )
                    .locale(words),
                ),
            ));
        }

        // No touchpad: the sentence and a way to look again.
        if self.model.devices().is_empty() {
            rows.push(Row::new(
                0.0,
                SettingsCard::new("touchpad-none", c.no_devices)
                    .icon(Picture::Touchpad)
                    .content(
                        Button::standard("touchpad-none-refresh", c.refresh)
                            .icon(Picture::Refresh)
                            .on_click(Self::act(cx, |this, cx| this.refresh(cx))),
                    ),
            ));
            return rows;
        }

        rows.push(Row::card(
            0.0,
            card::THIS_TOUCHPAD,
            self.this_touchpad_card(cx),
        ));

        // The selector bar, with the page's one command beside it.
        let items = SubPage::ALL
            .iter()
            .map(|page| SelectorItem::new(page.label(locale)))
            .collect();
        rows.push(Row::new(
            16.0,
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(
                    div().min_w_0().overflow_x_hidden().child(
                        SelectorBar::new("touchpad-sub-pages", items, Some(self.sub_page.index()))
                            .on_select(Self::act_with(cx, |this, index: usize, cx| {
                                if let Some(page) = SubPage::ALL.get(index) {
                                    this.show(*page, cx);
                                }
                            })),
                    ),
                ),
        ));

        if let Some((state, text)) = self.model.result_banner() {
            let severity = match state {
                RunState::Applied => Severity::Success,
                RunState::NothingToDo => Severity::Informational,
                RunState::AwaitingSignOut | RunState::PartiallySupported => Severity::Warning,
                RunState::Failed => Severity::Error,
            };
            rows.push(Row::new(
                12.0,
                InfoBar::new("touchpad-result", severity, text).locale(words),
            ));
        }

        let mut body = match self.sub_page {
            SubPage::Pointer => self.section_rows(Section::Pointer, cx),
            SubPage::Scrolling => self.section_rows(Section::Scrolling, cx),
            SubPage::Clicking => self.section_rows(Section::Clicking, cx),
            SubPage::Gestures => self.gesture_rows(cx),
            SubPage::Devices => self.device_rows(cx),
            SubPage::Diagnostics => self.diagnostics_rows(cx),
        };
        if let Some(first) = body.first_mut() {
            first.set_gap(16.0);
        }
        rows.extend(body);
        rows
    }

    /// The Apply bar: shown only while something is staged.
    pub(crate) fn apply_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let staged = view::staged_count(&self.model);
        let text = view::apply_bar_text(self.locale, staged)?;
        let c = self.c();
        Some(
            StagedChangesBar::new("touchpad-apply-bar", staged, text)
                .primary(
                    c.apply,
                    Self::act(cx, |this, cx| {
                        this.apply(cx);
                    }),
                )
                .secondary(c.discard, Self::act(cx, |this, cx| this.discard(cx)))
                .busy(self.busy, Some(c.busy))
                .into_any_element(),
        )
    }

    fn this_touchpad_card(&self, cx: &mut Context<Self>) -> SettingsExpander {
        let c = self.c();
        let pc = page_copy(self.locale);
        let facts = view::this_touchpad(&self.model, self.locale);
        let adapter = view::adapter_words(&self.gestures.adapter().describe(), self.locale);
        let health = facts.health;
        SettingsExpander::new("touchpad-this-card", pc.this_touchpad, self.this_expanded)
            .icon(Picture::Touchpad)
            .description(format!("{} · {health}", facts.device))
            // The page's one command: read every value and device again. It
            // sits on the card it refreshes, so the selector bar keeps its
            // whole width at 960px in either language.
            .content(
                Button::standard("touchpad-refresh", c.refresh)
                    .icon(Picture::Refresh)
                    .on_click(Self::act(cx, |this, cx| this.refresh(cx))),
            )
            .on_toggle(Self::act_with(cx, |this, open: bool, cx| {
                this.this_expanded = open;
                cx.notify();
            }))
            .row(SettingsRow::new(c.selected_touchpad).content(value(facts.device, cx)))
            .row(SettingsRow::new(c.session).content(value(facts.session, cx)))
            .row(SettingsRow::new(c.backend).content(value(facts.backend, cx)))
            .row(SettingsRow::new(c.health).content(value(health, cx)))
            .row(SettingsRow::new(pc.gestures_row).content(value(adapter, cx)))
            .row(SettingsRow::new(c.pointer_summary).content(value(facts.pointer, cx)))
            .row(SettingsRow::new(c.scroll_summary).content(value(facts.scroll, cx)))
    }

    /// Pointer, Scrolling, or Clicking: one card per setting, the section's
    /// extra cards, and Restore this section.
    fn section_rows(&self, section: Section, cx: &mut Context<Self>) -> Vec<Row> {
        let c = self.c();
        let setting_rows = self.model.rows(section);
        let mut rows = Vec::new();
        let mut gap = 0.0;
        for row in &setting_rows {
            if row.setting == SettingId::VerticalScrollFactor {
                rows.push(Row::new(
                    if rows.is_empty() { 0.0 } else { GROUP_GAP },
                    label(c.scroll_sensitivity, BODY_STRONG, palette(cx).text_primary),
                ));
                gap = FIRST_CARD_GAP;
            }
            rows.push(Row::card(
                gap,
                row.setting.key(),
                self.setting_card(row, cx),
            ));
            gap = CARD_GAP;
            if row.setting == SettingId::HorizontalScrollFactor {
                let linked = self.model.config().scrolling.linked_axes;
                rows.push(Row::card(
                    CARD_GAP,
                    card::LINKED_AXES,
                    SettingsCard::new(card::LINKED_AXES, c.linked_axes).content(
                        ToggleSwitch::new("touchpad-linked-axes-switch", linked)
                            .locale(self.locale.words())
                            .on_change(Self::act_with(cx, |this, on: bool, cx| {
                                this.stage_linked_axes(on, cx)
                            })),
                    ),
                ));
                gap = GROUP_GAP;
            }
        }
        match section {
            Section::Pointer => rows.push(Row::card(
                GROUP_GAP,
                card::POINTER_TEST,
                self.pointer_test(cx),
            )),
            Section::Scrolling => rows.push(Row::card(
                GROUP_GAP,
                card::SCROLL_TEST,
                self.scroll_test(cx),
            )),
            Section::Clicking => {}
        }
        rows.push(Row::new(
            GROUP_GAP,
            div().flex().child(
                Button::standard("touchpad-restore-section", c.restore_section)
                    .icon(Picture::Restore)
                    .disabled(self.model.state().backup().is_none() || self.model.safe_mode())
                    .on_click(Self::act(cx, move |this, cx| {
                        this.restore_section(section, cx);
                    })),
            ),
        ));
        rows
    }

    /// One setting. The single place that decides between a working control
    /// and an explanation.
    fn setting_card(&self, row: &SettingRow, cx: &mut Context<Self>) -> SettingsCard {
        let c = self.c();
        let setting = row.setting;
        let card = SettingsCard::new(ElementId::Name(setting.key().into()), row.label);
        if !row.available {
            return card
                .disabled(true)
                .description(row.unavailable_reason.clone().unwrap_or_default());
        }
        let mut description = format!(
            "{}：{} · {}：{}",
            c.requested_value, row.requested_label, c.effective_value, row.effective_label
        );
        if view::views_locale(self.locale) == touchpad_views::Locale::EnUs {
            description = format!(
                "{}: {} · {}: {}",
                c.requested_value, row.requested_label, c.effective_value, row.effective_label
            );
        }
        if let Some(result) = &row.result {
            description.push_str(" · ");
            description.push_str(result);
        }
        let disabled = self.model.safe_mode();
        let control: AnyElement = match row.control {
            Control::Slider { min, max, step } => {
                let model = SliderModel {
                    step,
                    large_change: step * 4.0,
                    ..SliderModel::new(min, max)
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        Slider::new(
                            ElementId::Name(format!("{}-slider", setting.key()).into()),
                            row.requested.as_number().unwrap_or(min),
                            model,
                        )
                        .width(px(SLIDER_WIDTH))
                        .disabled(disabled)
                        .on_change(Self::act_with(cx, move |this, value: f64, cx| {
                            this.stage_number(setting, value, cx)
                        })),
                    )
                    .child(
                        label(row.requested_label.clone(), BODY, palette(cx).text_primary)
                            .w(px(48.0))
                            .text_right(),
                    )
                    .into_any_element()
            }
            Control::Switch => ToggleSwitch::new(
                ElementId::Name(format!("{}-switch", setting.key()).into()),
                row.requested.as_bool().unwrap_or(false),
            )
            .locale(self.locale.words())
            .disabled(disabled)
            .on_change(Self::act_with(cx, move |this, on: bool, cx| {
                this.stage_value(setting, SettingValue::toggle(on), cx)
            }))
            .into_any_element(),
            Control::Choice => {
                let options = choices(setting, c);
                let selected = options
                    .iter()
                    .position(|(value, _)| *value == row.requested);
                let values: Vec<SettingValue> = options.iter().map(|(value, _)| *value).collect();
                ComboBox::new(
                    ElementId::Name(format!("{}-choice", setting.key()).into()),
                    options
                        .iter()
                        .map(|(_, label)| ComboItem::new(*label))
                        .collect(),
                    selected,
                )
                .min_width(px(CHOICE_WIDTH))
                .disabled(disabled)
                .on_select(Self::act_with(cx, move |this, index: usize, cx| {
                    if let Some(value) = values.get(index) {
                        this.stage_value(setting, *value, cx);
                    }
                }))
                .into_any_element()
            }
        };
        let mut badges = div().flex().flex_wrap().gap(px(4.0));
        if row.pending {
            badges = badges.child(Chip::new(c.pending_badge));
        }
        if row.drifted {
            badges = badges.child(Chip::new(c.drifted_badge));
        }
        if row.needs_sign_out {
            badges = badges.child(Chip::new(c.sign_out_badge));
        }
        card.description(description).content(
            div()
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(badges)
                .child(control),
        )
    }

    /// The pointer test surface: a bounded box that shows where the pointer
    /// is. It performs no system action; moving over it is how the
    /// sensitivity is felt before anything is applied.
    fn pointer_test(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.c();
        let p = palette(cx);
        let trace = self.pointer;
        let surface = self.surface.clone();
        let weak = cx.entity().downgrade();
        test_card(
            c.pointer_test_title,
            c.pointer_test_hint,
            div()
                .id("touchpad-pointer-surface")
                .relative()
                .w_full()
                .h(px(180.0))
                .rounded(radius::SM)
                .border_1()
                .border_color(p.card_stroke)
                .bg(p.card_secondary)
                .overflow_hidden()
                .child(
                    canvas(move |bounds, _, _| surface.set(bounds), |_, _, _, _| {})
                        .absolute()
                        .size_full(),
                )
                .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                    let _ = weak.update(cx, |this, cx| this.trace_pointer(event.position, cx));
                })
                .when(!trace.inside, |this| {
                    this.child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(label(c.pointer_test_idle, CAPTION, p.text_secondary)),
                    )
                })
                .when(trace.inside, |this| {
                    this.child(
                        div()
                            .absolute()
                            .left(relative(trace.x))
                            .top(relative(trace.y))
                            .size(px(12.0))
                            .rounded_full()
                            .bg(p.accent),
                    )
                })
                .into_any_element(),
            cx,
        )
    }

    /// The scroll test area: real content that really scrolls, both ways.
    fn scroll_test(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.c();
        let p = palette(cx);
        test_card(
            c.scroll_test_title,
            c.scroll_test_hint,
            div()
                .id("touchpad-scroll-area")
                .w_full()
                .h(px(200.0))
                .rounded(radius::SM)
                .border_1()
                .border_color(p.card_stroke)
                .bg(p.card_secondary)
                .overflow_scroll()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .p(px(12.0))
                        .gap(px(8.0))
                        .children((0..40).map(|row| {
                            div()
                                .flex()
                                .gap(px(12.0))
                                .children((0..14).map(move |column| {
                                    div().flex_none().w(px(96.0)).child(label(
                                        format!("{row:02}·{column:02}"),
                                        CAPTION,
                                        p.text_secondary,
                                    ))
                                }))
                        })),
                )
                .into_any_element(),
            cx,
        )
    }

    /// Devices: one card per touchpad, with its identity and connection under
    /// Details (audit T4: identifiers live here and nowhere else).
    fn device_rows(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let c = self.c();
        let pc = page_copy(self.locale);
        let selected = self
            .model
            .selected_device()
            .map(|device| device.identity.clone());
        let mut rows = Vec::new();
        for (index, device) in self.model.devices().iter().enumerate() {
            let is_selected = selected.as_deref() == Some(device.identity.as_str());
            let state = match device.state {
                DeviceState::Connected => c.state_connected,
                DeviceState::Disconnected => c.state_disconnected,
                DeviceState::Inhibited => c.state_inhibited,
            };
            let colon = if view::views_locale(self.locale) == touchpad_views::Locale::ZhTw {
                "："
            } else {
                ": "
            };
            let capabilities = format!(
                "{}{colon}{} · {}",
                c.contacts,
                device.capabilities.max_contacts,
                if device.capabilities.buttonpad {
                    c.buttonpad
                } else {
                    c.separate_buttons
                }
            );
            let mut expander = SettingsExpander::new(
                ElementId::named_usize("touchpad-device", index),
                view::friendly_name(&device.name),
                self.device_details == Some(index),
            )
            .icon(Picture::Touchpad)
            .description(format!("{state} · {capabilities}"))
            .on_toggle(Self::act_with(cx, move |this, open: bool, cx| {
                this.device_details = open.then_some(index);
                cx.notify();
            }))
            .row(
                SettingsRow::new(pc.details)
                    .description(c.device_identity)
                    .content(value(
                        format!(
                            "{} · {} · {}:{} · {}",
                            device.name, device.identity, device.vendor, device.product, device.bus
                        ),
                        cx,
                    )),
            )
            .row(SettingsRow::new(c.device_capabilities).content(value(capabilities, cx)))
            .row(SettingsRow::new(c.device_scope).content(value(
                if is_selected {
                    c.scope_global
                } else {
                    c.scope_per_device
                },
                cx,
            )));
            if is_selected {
                expander = expander.content(Chip::new(pc.selected_badge));
            }
            rows.push(if index == 0 {
                Row::card(0.0, card::DEVICES, expander)
            } else {
                Row::new(CARD_GAP, expander)
            });
        }
        rows
    }

    /// Diagnostics: the checks, the effective values, the values captured
    /// before the first change, Restore all, and safe mode.
    fn diagnostics_rows(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let c = self.c();
        let pc = page_copy(self.locale);
        let locale = view::views_locale(self.locale);
        let health = self.model.health();
        let mut rows = Vec::new();

        rows.push(Row::new(
            0.0,
            label(c.health, BODY_STRONG, palette(cx).text_primary),
        ));
        let checks: Vec<Vec<TableCell>> = health
            .checks
            .iter()
            .map(|check| {
                vec![
                    TableCell::new(super::copy::check_words(&check.id, self.locale)),
                    TableCell::new(touchpad_views::i18n::health_words(check.state, locale)),
                    TableCell::new(check.detail.clone()),
                ]
            })
            .collect();
        rows.push(Row::new(
            FIRST_CARD_GAP,
            DetailsTable::new(
                "touchpad-checks",
                vec![
                    Column::new(pc.check_column).width(px(220.0)),
                    Column::new(pc.state_column).width(px(160.0)),
                    Column::new(pc.detail_column),
                ],
                checks,
            ),
        ));

        rows.push(self.group(c.effective_values, cx));
        let values: Vec<Vec<TableCell>> = self
            .model
            .all_rows()
            .iter()
            .map(|row| {
                vec![
                    TableCell::new(row.label),
                    TableCell::new(row.requested_label.clone()),
                    TableCell::new(row.effective_label.clone()),
                ]
            })
            .collect();
        rows.push(Row::card(
            FIRST_CARD_GAP,
            card::EFFECTIVE,
            DetailsTable::new(
                "touchpad-effective-table",
                vec![
                    Column::new(c.setting_key).width(px(260.0)),
                    Column::new(c.requested_value).width(px(200.0)),
                    Column::new(c.effective_value),
                ],
                values,
            ),
        ));

        rows.push(self.group(c.restore_actions, cx));
        let has_capture = self.model.state().backup().is_some();
        let captured: Vec<Vec<TableCell>> = self
            .model
            .restore_rows(RestoreScope::All)
            .into_iter()
            .map(|row| {
                vec![
                    TableCell::new(row.label),
                    TableCell::new(row.captured_label),
                    TableCell::new(row.detail.unwrap_or_default()),
                ]
            })
            .collect();
        let restore = SettingsCard::new(card::RESTORE, pc.captured_values)
            .icon(Picture::Restore)
            .description(if has_capture {
                c.captured_before
            } else {
                c.nothing_captured
            })
            .content(
                Button::standard("touchpad-restore-all", c.restore_all)
                    .disabled(!has_capture || self.model.safe_mode())
                    .on_click(Self::act(cx, |this, cx| {
                        this.restore(RestoreScope::All, cx);
                    })),
            );
        rows.push(Row::card(FIRST_CARD_GAP, card::RESTORE, restore));
        if has_capture {
            rows.push(Row::new(
                CARD_GAP,
                DetailsTable::new(
                    "touchpad-captured-table",
                    vec![
                        Column::new(c.setting_key).width(px(260.0)),
                        Column::new(c.previous_value).width(px(220.0)),
                        Column::new(pc.detail_column),
                    ],
                    captured,
                ),
            ));
        }
        rows.push(Row::card(
            CARD_GAP,
            card::SAFE_MODE,
            SettingsCard::new("touchpad-safe-mode-card", pc.safe_mode_card)
                .icon(Picture::Shield)
                .description(pc.safe_mode_description)
                .content(
                    ToggleSwitch::new("touchpad-safe-mode-switch", self.model.safe_mode())
                        .locale(self.locale.words())
                        .on_change(Self::act_with(cx, |this, on: bool, cx| {
                            this.set_safe_mode(on, cx)
                        })),
                ),
        ));

        // Recognized gesture events and conflict results, and no raw input:
        // a recognized event is a gesture, a phase, and a percentage.
        rows.push(self.group(c.gestures_title, cx));
        let lines = self.gestures.diagnostics_lines(c);
        let adapter = view::adapter_words(&self.gestures.adapter().describe(), self.locale);
        let mut gestures = SettingsExpander::new("touchpad-gesture-diagnostics", adapter, false)
            .icon(Picture::Gesture)
            .description(self.gestures.preset_card(c).status_label);
        if lines.is_empty() {
            gestures = gestures.row(SettingsRow::new(c.test_no_events));
        }
        for line in lines {
            gestures = gestures.row(SettingsRow::new(line));
        }
        rows.push(Row::new(FIRST_CARD_GAP, gestures));
        rows
    }
}

/// A value on the trailing side of a settings row, in secondary text.
pub(super) fn value(text: impl Into<SharedString>, cx: &App) -> Div {
    label(text, BODY, palette(cx).text_secondary)
        .text_right()
        .max_w(px(420.0))
}

/// A card that holds a test area under its header and hint.
fn test_card(title: &'static str, hint: &'static str, area: AnyElement, cx: &App) -> AnyElement {
    let p = palette(cx);
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .p(px(16.0))
        .rounded(radius::SM)
        .border_1()
        .border_color(p.card_stroke)
        .bg(p.card)
        .child(label(title, BODY, p.text_primary))
        .child(label(hint, CAPTION, p.text_secondary))
        .child(area)
        .into_any_element()
}

/// The options of a choice setting, in the order the older window showed
/// them.
pub(super) fn choices(setting: SettingId, c: &'static TpCopy) -> Vec<(SettingValue, &'static str)> {
    match setting {
        SettingId::AccelerationProfile => AccelerationProfile::ALL
            .into_iter()
            .map(|profile| {
                (
                    SettingValue::acceleration(profile),
                    match profile {
                        AccelerationProfile::Default => c.profile_default,
                        AccelerationProfile::Adaptive => c.profile_adaptive,
                        AccelerationProfile::Flat => c.profile_flat,
                    },
                )
            })
            .collect(),
        _ => ClickMethod::ALL
            .into_iter()
            .map(|method| {
                (
                    SettingValue::click(method),
                    match method {
                        ClickMethod::Default => c.method_default,
                        ClickMethod::Areas => c.method_areas,
                        ClickMethod::Fingers => c.method_fingers,
                        ClickMethod::None => c.method_none,
                    },
                )
            })
            .collect(),
    }
}

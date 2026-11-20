//! Draws System (spec section 4) from what `hardware-core` and `decide`
//! say, with the Fluent kit only.

use better_ui::fluent::button::Button;
use better_ui::fluent::info_bar::{ErrorBar, ErrorReport, InfoBar, Severity};
use better_ui::fluent::progress::{Progress, ProgressRing};
use better_ui::fluent::selection::{ComboBox, ComboItem, Slider, SliderModel};
use better_ui::fluent::settings_card::{SettingsCard, SettingsExpander, SettingsRow};
use better_ui::fluent::toggle::{RadioButtons, RadioOption, ToggleSwitch};
use better_ui::fluent::{BODY, CAPTION, label, palette};
use better_ui::icon_set::{Glyph, Picture};
use gpui::*;
use gpui_component::IconName;
use hardware_core::Card;
use hardware_core::charge::ChargeChoice;
use hardware_core::fan::{FanMode, FanState};
use hardware_core::follow::ChargerFollow;
use hardware_core::page::{CoolingGroup, DisplayGroup, PowerGroup, system_view};
use hardware_core::power::Profile;
use hardware_core::sensors::summary;
use hardware_core::write::WriteOutcome;

use super::card;
use super::copy::copy;
use super::decide::{self, outcome_state};
use super::state::{Group, Writing};
use crate::app::ManagerApp;
use crate::i18n::shell;
use crate::pages::states::{PageState, describe};
use crate::render::{CARD_GAP, FIRST_CARD_GAP, Row};

/// The width of the page's lists and sliders, so they line up.
const CONTROL_WIDTH: f32 = 260.0;

impl ManagerApp {
    pub(crate) fn system_rows(&mut self, cx: &mut Context<Self>) -> Vec<Row> {
        let locale = self.locale;
        let c = copy(locale);
        let p = palette(cx);
        let mut rows = vec![self.title_row(shell(locale).system, cx)];

        let Some(facts) = self.system.facts.clone() else {
            if self.system.shows_ring() {
                let text = describe(locale, &PageState::Loading { elapsed_ms: 300 }).text;
                rows.push(Row::new(
                    0.0,
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .child(
                            ProgressRing::new("system-reading", Progress::Indeterminate)
                                .size(px(32.0)),
                        )
                        .child(label(text, CAPTION, p.text_secondary)),
                ));
            }
            return rows;
        };
        let view = system_view(&facts);
        if view.is_empty() {
            rows.push(Row::new(
                0.0,
                label(c.empty, BODY, p.text_secondary).pb(px(4.0)),
            ));
        }
        if let Some(power) = &view.power {
            rows.push(self.group_header(c.group_power, cx));
            rows.extend(self.bar_rows(Group::Power, cx));
            rows.extend(self.power_rows(power, cx));
        }
        if let Some(display) = &view.display {
            rows.push(self.group_header(c.group_display, cx));
            rows.extend(self.bar_rows(Group::Display, cx));
            rows.extend(self.display_rows(display, cx));
        }
        if let Some(cooling) = &view.cooling {
            rows.push(self.group_header(c.group_cooling, cx));
            rows.extend(self.bar_rows(Group::Cooling, cx));
            rows.extend(self.cooling_rows(cooling, cx));
        }
        rows.extend(self.drive_rows(cx));
        rows.push(self.group_header(c.group_about, cx));
        rows.push(Row::card(
            FIRST_CARD_GAP,
            card::ABOUT,
            self.about_expander(cx),
        ));
        rows
    }

    /// The bar a write left over `group`, if any (spec section 2).
    fn bar_rows(&self, group: Group, cx: &mut Context<Self>) -> Vec<Row> {
        let Some((at, outcome)) = &self.system.bar else {
            return Vec::new();
        };
        if *at != group {
            return Vec::new();
        }
        let locale = self.locale;
        let Some(state) = outcome_state(outcome, locale) else {
            return Vec::new();
        };
        let words = describe(locale, &state);
        let close = Self::act(cx, |this, _, cx| {
            this.system.bar = None;
            cx.notify();
        });
        let element = match (&state, outcome) {
            // No close button: the state stays true until the next action.
            (PageState::NoPermission, _) => {
                InfoBar::new("system-bar", Severity::Warning, words.text)
                    .locale(locale.words())
                    .into_any_element()
            }
            (PageState::NotAllowed, _) => InfoBar::new("system-bar", Severity::Error, words.text)
                .locale(locale.words())
                .on_close(close)
                .into_any_element(),
            (PageState::ReadBackDifferent { .. }, _) => {
                InfoBar::new("system-bar", Severity::Warning, words.text)
                    .locale(locale.words())
                    .on_close(close)
                    .into_any_element()
            }
            (_, WriteOutcome::Failed { details }) => ErrorBar::new(
                "system-bar",
                ErrorReport::new(
                    words.text.trim_end().to_string(),
                    crate::reasons::details_in_words(details, locale),
                ),
            )
            .locale(locale.words())
            .on_close(close)
            .into_any_element(),
            _ => return Vec::new(),
        };
        vec![Row::new(FIRST_CARD_GAP, element)]
    }

    fn unreadable(&self) -> String {
        describe(self.locale, &PageState::Unreadable).text
    }

    fn power_rows(&self, power: &PowerGroup, cx: &mut Context<Self>) -> Vec<Row> {
        let c = copy(self.locale);
        let mut rows = Vec::new();
        let mut gap = FIRST_CARD_GAP;
        let writing = self.system.writing;

        match &power.mode {
            Card::Hidden => {}
            Card::Disabled { .. } => {
                rows.push(Row::card(
                    gap,
                    card::POWER_MODE,
                    SettingsCard::new(card::POWER_MODE, c.power_mode)
                        .icon(Picture::Gauge)
                        .description(self.unreadable())
                        .disabled(true),
                ));
                gap = CARD_GAP;
            }
            Card::Live(mode) => {
                let options = mode.options.clone();
                let items = options
                    .iter()
                    .map(|p| ComboItem::new(decide::profile_label(*p, c)))
                    .collect();
                let selected = mode
                    .selected
                    .and_then(|s| options.iter().position(|o| *o == s));
                let mut card_ = SettingsCard::new(card::POWER_MODE, c.power_mode)
                    .icon(Picture::Gauge)
                    .description(c.power_mode_description)
                    .content(
                        ComboBox::new("system-power-mode-box", items, selected)
                            .min_width(px(CONTROL_WIDTH))
                            .disabled(writing == Some(Writing::PowerMode))
                            .on_select(Self::act_with(cx, move |this, index: usize, _, cx| {
                                if let Some(profile) = options.get(index) {
                                    this.set_power_mode(*profile, cx);
                                }
                            })),
                    );
                for caution in &mode.cautions {
                    card_ = card_.caution(decide::caution_line(caution, c));
                }
                rows.push(Row::card(gap, card::POWER_MODE, card_));
                gap = CARD_GAP;
                if power.follow_charger {
                    rows.push(Row::card(
                        gap,
                        card::FOLLOW,
                        self.follow_expander(&mode.options, cx),
                    ));
                }
            }
        }

        match &power.charge_limit {
            Card::Hidden => {}
            Card::Disabled { .. } => {
                rows.push(Row::card(
                    gap,
                    card::CHARGE_LIMIT,
                    SettingsCard::new(card::CHARGE_LIMIT, c.charge_limit)
                        .icon(Picture::BatteryMedium)
                        .description(self.unreadable())
                        .disabled(true),
                ));
                gap = CARD_GAP;
            }
            Card::Live(view) => {
                let choices = view.choices.clone();
                let items = choices
                    .iter()
                    .map(|choice| {
                        ComboItem::new(decide::charge_label(*choice, c))
                            // A value set elsewhere is shown, never chosen.
                            .disabled(matches!(choice, ChargeChoice::SetElsewhere(_)))
                    })
                    .collect();
                let description = decide::sentences(
                    self.locale,
                    &[
                        c.charge_limit_description,
                        c.charge_at_startup,
                        c.charge_auth_note,
                    ],
                );
                rows.push(Row::card(
                    gap,
                    card::CHARGE_LIMIT,
                    SettingsCard::new(card::CHARGE_LIMIT, c.charge_limit)
                        .icon(Picture::BatteryMedium)
                        .description(description)
                        .content(
                            ComboBox::new("system-charge-limit-box", items, Some(view.selected))
                                .min_width(px(CONTROL_WIDTH))
                                .disabled(writing == Some(Writing::ChargeLimit))
                                .on_select(Self::act_with(cx, move |this, index: usize, _, cx| {
                                    if let Some(ChargeChoice::Limit(percent)) = choices.get(index) {
                                        this.set_charge_limit(*percent, cx);
                                    }
                                })),
                        ),
                ));
                gap = CARD_GAP;
            }
        }

        match &power.battery {
            Card::Hidden => {}
            Card::Disabled { .. } => {
                rows.push(Row::card(
                    gap,
                    card::BATTERY,
                    SettingsCard::new(card::BATTERY, c.battery)
                        .icon(Picture::Battery)
                        .description(self.unreadable())
                        .disabled(true),
                ));
            }
            Card::Live(battery) => {
                let mut expander =
                    SettingsExpander::new(card::BATTERY, c.battery, self.system.battery_open)
                        .icon(match battery.capacity {
                            Some(p) if p >= 90 => Picture::BatteryFull,
                            Some(p) if p >= 40 => Picture::BatteryMedium,
                            Some(_) => Picture::BatteryLow,
                            None => Picture::Battery,
                        })
                        .description(decide::battery_description(battery, self.locale))
                        .on_toggle(Self::act_with(cx, |this, open: bool, _, cx| {
                            this.system.battery_open = open;
                            cx.notify();
                        }));
                for line in decide::battery_rows(battery, c) {
                    expander = expander.row(SettingsRow::new(line));
                }
                rows.push(Row::card(gap, card::BATTERY, expander));
            }
        }
        rows
    }

    /// "Switch power mode with the charger": the switch, and the two modes
    /// it sets once it is on.
    fn follow_expander(&self, offered: &[Profile], cx: &mut Context<Self>) -> SettingsExpander {
        let c = copy(self.locale);
        let (follow, broken) = match &self.system.follow {
            Some(Ok(follow)) => (*follow, false),
            Some(Err(_)) => (ChargerFollow::default(), true),
            None => (ChargerFollow::default(), false),
        };
        let busy = self.system.writing == Some(Writing::Follow);
        let mut expander =
            SettingsExpander::new(card::FOLLOW, c.follow_header, self.system.follow_open)
                .icon(Picture::BatteryCharging)
                .description(if broken {
                    c.follow_unreadable
                } else {
                    c.follow_description
                })
                .content(
                    ToggleSwitch::new("system-follow-switch", follow.enabled)
                        .locale(self.locale.words())
                        .disabled(busy)
                        .on_change(Self::act_with(cx, move |this, on: bool, _, cx| {
                            this.system.follow_open = on;
                            this.set_follow(
                                ChargerFollow {
                                    enabled: on,
                                    ..follow
                                },
                                cx,
                            );
                        })),
                )
                .on_toggle(Self::act_with(cx, |this, open: bool, _, cx| {
                    this.system.follow_open = open;
                    cx.notify();
                }));
        for (id, header, current, plugged) in [
            (
                "system-follow-plugged",
                c.follow_plugged_in,
                follow.plugged_in,
                true,
            ),
            (
                "system-follow-battery",
                c.follow_on_battery,
                follow.on_battery,
                false,
            ),
        ] {
            let options = offered.to_vec();
            let items = options
                .iter()
                .map(|p| ComboItem::new(decide::profile_label(*p, c)))
                .collect();
            let selected = options.iter().position(|p| *p == current);
            expander = expander.row(
                SettingsRow::new(header).content(
                    ComboBox::new(id, items, selected)
                        .min_width(px(CONTROL_WIDTH))
                        .disabled(busy || !follow.enabled)
                        .on_select(Self::act_with(cx, move |this, index: usize, _, cx| {
                            if let Some(profile) = options.get(index) {
                                let next = if plugged {
                                    ChargerFollow {
                                        plugged_in: *profile,
                                        ..follow
                                    }
                                } else {
                                    ChargerFollow {
                                        on_battery: *profile,
                                        ..follow
                                    }
                                };
                                this.set_follow(next, cx);
                            }
                        })),
                ),
            );
        }
        expander
    }

    fn display_rows(&self, display: &DisplayGroup, cx: &mut Context<Self>) -> Vec<Row> {
        let c = copy(self.locale);
        let p = palette(cx);
        let mut rows = Vec::new();
        let mut gap = FIRST_CARD_GAP;
        match &display.brightness {
            Card::Hidden => {}
            Card::Disabled { .. } => {
                rows.push(Row::card(
                    gap,
                    card::BRIGHTNESS,
                    SettingsCard::new(card::BRIGHTNESS, c.brightness)
                        .icon(Picture::Brightness)
                        .description(self.unreadable())
                        .disabled(true),
                ));
                gap = CARD_GAP;
            }
            Card::Live(value) => {
                let shown = self.system.brightness_drag.unwrap_or(*value);
                rows.push(Row::card(
                    gap,
                    card::BRIGHTNESS,
                    SettingsCard::new(card::BRIGHTNESS, c.brightness)
                        .icon(Picture::Brightness)
                        .description(c.brightness_description)
                        .content(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(12.0))
                                .child(
                                    Slider::new(
                                        "system-brightness-slider",
                                        f64::from(shown),
                                        SliderModel::new(0.0, 100.0),
                                    )
                                    .width(px(CONTROL_WIDTH - 52.0))
                                    .on_change(Self::act_with(cx, |this, v: f64, _, cx| {
                                        this.drag_brightness(v.round().clamp(0.0, 100.0) as u8, cx)
                                    })),
                                )
                                .child(div().w(px(40.0)).flex().justify_end().child(label(
                                    format!("{shown}"),
                                    BODY,
                                    p.text_primary,
                                ))),
                        ),
                ));
                gap = CARD_GAP;
            }
        }
        match &display.keyboard {
            Card::Hidden => {}
            Card::Disabled { .. } => {
                rows.push(Row::card(
                    gap,
                    card::KEYBOARD,
                    SettingsCard::new(card::KEYBOARD, c.keyboard)
                        .icon(Picture::Keyboard)
                        .description(self.unreadable())
                        .disabled(true),
                ));
            }
            Card::Live(levels) => {
                let levels = *levels;
                let shown = self
                    .system
                    .keyboard_drag
                    .unwrap_or(levels.level)
                    .min(levels.levels);
                let model = SliderModel {
                    minimum: 0.0,
                    maximum: f64::from(levels.levels),
                    step: 1.0,
                    large_change: 1.0,
                };
                rows.push(Row::card(
                    gap,
                    card::KEYBOARD,
                    SettingsCard::new(card::KEYBOARD, c.keyboard)
                        .icon(Picture::Keyboard)
                        .description(decide::keyboard_description(&levels, c))
                        .content(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(12.0))
                                .child(
                                    Slider::new("system-keyboard-slider", f64::from(shown), model)
                                        .width(px(CONTROL_WIDTH - 52.0))
                                        .on_change(Self::act_with(
                                            cx,
                                            move |this, v: f64, _, cx| {
                                                let level = v.round().max(0.0) as u32;
                                                if this.system.keyboard_drag != Some(level) {
                                                    this.system.keyboard_drag = Some(level);
                                                    this.set_keyboard(levels, level, cx);
                                                }
                                            },
                                        )),
                                )
                                .child(div().w(px(40.0)).flex().justify_end().child(label(
                                    decide::keyboard_value(shown, c),
                                    BODY,
                                    p.text_primary,
                                ))),
                        ),
                ));
            }
        }
        // The keyboard's colour (ticket 136), right under its backlight.
        let gap = if rows.is_empty() {
            FIRST_CARD_GAP
        } else {
            CARD_GAP
        };
        rows.extend(self.lighting_rows(&display.lighting, gap, cx));
        rows
    }

    fn cooling_rows(&self, cooling: &CoolingGroup, cx: &mut Context<Self>) -> Vec<Row> {
        let c = copy(self.locale);
        let mut rows = Vec::new();
        let mut gap = FIRST_CARD_GAP;
        match &cooling.fan {
            Card::Hidden => {}
            Card::Disabled { .. } => {
                rows.push(Row::card(
                    gap,
                    card::FAN_MODE,
                    SettingsCard::new(card::FAN_MODE, c.fan_mode)
                        .icon(Picture::Fan)
                        .description(self.unreadable())
                        .disabled(true),
                ));
                gap = CARD_GAP;
            }
            Card::Live(state) => {
                let selected = state
                    .mode()
                    .and_then(|m| FanMode::ALL.iter().position(|o| *o == m));
                let options = FanMode::ALL
                    .iter()
                    .map(|m| RadioOption::new(decide::fan_label(*m, c)))
                    .collect();
                let mut card_ = SettingsCard::new(card::FAN_MODE, c.fan_mode)
                    .icon(Picture::Fan)
                    .description(decide::sentences(
                        self.locale,
                        &[c.fan_mode_description, c.fan_after_restart],
                    ))
                    .content(
                        RadioButtons::new("system-fan-mode-radios", options, selected)
                            .horizontal()
                            .disabled(self.system.writing == Some(Writing::FanMode))
                            .on_select(Self::act_with(cx, move |this, index: usize, _, cx| {
                                if let Some(mode) = FanMode::ALL.get(index)
                                    && selected != Some(index)
                                {
                                    this.set_fan_mode(*mode, cx);
                                }
                            })),
                    );
                if let FanState::Other(value) = state {
                    card_ = card_.caution(
                        describe(
                            self.locale,
                            &PageState::ReadBackDifferent {
                                value: value.to_string(),
                            },
                        )
                        .text,
                    );
                }
                rows.push(Row::card(gap, card::FAN_MODE, card_));
                gap = CARD_GAP;
            }
        }
        if !cooling.sensors.is_empty() {
            let mut expander =
                SettingsExpander::new(card::SENSORS, c.sensors, self.system.sensors_open)
                    .icon(Picture::Thermometer)
                    .description(decide::sensors_description(&summary(&cooling.sensors), c))
                    .on_toggle(Self::act_with(cx, |this, open: bool, _, cx| {
                        this.system.sensors_open = open;
                        cx.notify();
                    }));
            for row in &cooling.sensors {
                expander = expander.row(
                    SettingsRow::new(decide::sensor_label(&row.label, c)).content(label(
                        decide::sensor_value(row, c),
                        BODY,
                        palette(cx).text_secondary,
                    )),
                );
            }
            rows.push(Row::card(gap, card::SENSORS, expander));
        }
        rows
    }

    fn about_expander(&self, cx: &mut Context<Self>) -> SettingsExpander {
        let c = copy(self.locale);
        let mut expander = SettingsExpander::new(card::ABOUT, c.about, self.system.about_open)
            .icon(Picture::About)
            .description(
                self.system
                    .about
                    .product
                    .clone()
                    .unwrap_or_else(|| c.about_unknown.to_string()),
            )
            .on_toggle(Self::act_with(cx, |this, open: bool, _, cx| {
                this.system.about_open = open;
                cx.notify();
            }));
        for (index, (name, value)) in decide::about_rows(&self.system.about, self.locale)
            .into_iter()
            .enumerate()
        {
            let copied = self.system.copied == Some(index);
            let text = value.clone();
            expander = expander.row(
                SettingsRow::new(name).description(value).content(
                    Button::icon_button(
                        ElementId::named_usize("system-about-copy", index),
                        if copied {
                            Glyph::from(IconName::Check)
                        } else {
                            Glyph::from(Picture::Copy)
                        },
                        if copied { c.copied } else { c.copy },
                    )
                    .on_click(Self::act(cx, move |this, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        this.system.copied = Some(index);
                        cx.notify();
                    })),
                ),
            );
        }
        expander
    }

    /// Home's Power mode card (spec section 3), live: not drawn without
    /// power-profiles-daemon.
    pub(crate) fn power_mode_quick_card(&self, cx: &mut Context<Self>) -> Option<SettingsCard> {
        let facts = self.system.facts.as_ref()?;
        let view = system_view(facts);
        let Card::Live(mode) = view.power?.mode else {
            return None;
        };
        let c = copy(self.locale);
        let options = mode.options.clone();
        let items = options
            .iter()
            .map(|p| ComboItem::new(decide::profile_label(*p, c)))
            .collect();
        let selected = mode
            .selected
            .and_then(|s| options.iter().position(|o| *o == s));
        let mut card_ = SettingsCard::new("home-power-mode", c.power_mode)
            .icon(Picture::Gauge)
            .description(c.power_mode_description)
            .content(
                ComboBox::new("home-power-mode-box", items, selected)
                    .min_width(px(CONTROL_WIDTH))
                    .disabled(self.system.writing == Some(Writing::PowerMode))
                    .on_select(Self::act_with(cx, move |this, index: usize, _, cx| {
                        if let Some(profile) = options.get(index) {
                            this.set_power_mode(*profile, cx);
                        }
                    })),
            );
        for caution in &mode.cautions {
            card_ = card_.caution(decide::caution_line(caution, c));
        }
        Some(card_)
    }
}

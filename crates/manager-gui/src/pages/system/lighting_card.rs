//! The keyboard colour card (ticket 136, ADR 0034): what it keeps, its reads
//! and writes through `manager-daemon`, and how it is drawn with the Fluent
//! kit. What the card shows is `hardware_core::lighting::view`'s; the words
//! are `lighting`'s.

use better_ui::fluent::selection::{ComboBox, ComboItem, Slider, SliderModel};
use better_ui::fluent::settings_card::{SettingsCard, SettingsExpander, SettingsRow};
use better_ui::fluent::toggle::{RadioButtons, RadioOption};
use better_ui::fluent::{BODY, label, palette};
use gpui::*;
use gpui_component::IconName;
use hardware_core::Card;
use hardware_core::lighting::{
    self, Access, Effect, Interface, LightingChoice, LightingHint, LightingReply, LightingView,
    LiveLighting, Rgb, SWATCHES, Speed,
};
use hardware_core::write::WriteOutcome;
use hardware_platform::privileged;

use super::card;
use super::lighting::{self as words, copy};
use super::state::Group;
use crate::app::ManagerApp;
use crate::render::Row;

/// The width of the page's sliders, as the other cards draw them.
const SLIDER_WIDTH: f32 = 208.0;

#[derive(Default)]
pub(crate) struct LightingState {
    /// The daemon's answer; `None` until it lands.
    pub(crate) reply: Option<Result<LightingReply, Access>>,
    asked: bool,
    pub(crate) open: bool,
    pub(crate) writing: bool,
    /// The last choice asked for while another was being written.
    pending: Option<LightingChoice>,
    /// What the controls show while a change is on its way.
    pub(crate) shown: Option<LightingChoice>,
    task: Option<Task<()>>,
    write_task: Option<Task<()>>,
}

impl LightingState {
    pub(crate) fn blocks_restart(&self) -> bool {
        self.writing || self.pending.is_some()
    }
}

/// The system bus. A test build opens none, so no test reaches the
/// machine's daemon.
fn system_bus() -> Option<zbus::blocking::Connection> {
    if cfg!(test) {
        None
    } else {
        zbus::blocking::Connection::system().ok()
    }
}

/// A hue drag on a colour with no saturation would change nothing on the
/// keyboard: it starts from full saturation instead.
fn with_hue(colour: Rgb, hue: f64) -> Rgb {
    let (_, saturation) = colour.hue_saturation();
    let saturation = if saturation == 0.0 { 1.0 } else { saturation };
    Rgb::from_hue_saturation(hue, saturation)
}

fn with_saturation(colour: Rgb, saturation: f64) -> Rgb {
    let (hue, _) = colour.hue_saturation();
    Rgb::from_hue_saturation(hue, saturation)
}

impl ManagerApp {
    /// Asks the daemon about the keyboard once the page has found a colour
    /// interface in sysfs, and not again until something changes it.
    pub(crate) fn ensure_lighting(&mut self, cx: &mut Context<Self>) {
        let wanted = self
            .system
            .facts
            .as_ref()
            .and_then(|f| f.lighting.value())
            .is_some_and(|hint| hint.interface.is_some());
        if wanted && !self.system.lighting.asked {
            self.refresh_lighting(cx);
        }
    }

    pub(crate) fn refresh_lighting(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.system.lighting;
        if state.task.is_some() {
            return;
        }
        state.asked = true;
        state.task = Some(cx.spawn(async move |this, cx| {
            let reply = cx
                .background_spawn(async move {
                    match system_bus() {
                        Some(bus) => privileged::keyboard_lighting(&bus),
                        None => Err(Access::NoService),
                    }
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.system.lighting.task = None;
                app.system.lighting.reply = Some(reply);
                cx.notify();
            });
        }));
    }

    /// Writes a choice through the daemon. A drag moves faster than a write
    /// lands: the last choice asked for is written once the one on its way
    /// has, and the controls show it meanwhile.
    pub(crate) fn set_lighting(&mut self, choice: LightingChoice, cx: &mut Context<Self>) {
        let state = &mut self.system.lighting;
        state.shown = Some(choice);
        if state.writing {
            state.pending = Some(choice);
            cx.notify();
            return;
        }
        state.writing = true;
        if matches!(self.system.bar, Some((Group::Display, _))) {
            self.system.bar = None;
        }
        cx.notify();
        self.system.lighting.write_task = Some(cx.spawn(async move |this, cx| {
            let (outcome, reply) = cx
                .background_spawn(async move {
                    match system_bus() {
                        Some(bus) => privileged::set_keyboard_lighting(&bus, &choice),
                        None => (
                            WriteOutcome::Failed {
                                details: "the system bus is not reachable".to_string(),
                            },
                            None,
                        ),
                    }
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                let state = &mut app.system.lighting;
                state.writing = false;
                state.write_task = None;
                if let Some(reply) = reply {
                    state.reply = Some(Ok(reply));
                }
                let pending = state.pending.take();
                if outcome != WriteOutcome::Done {
                    app.system.bar = Some((Group::Display, outcome));
                }
                match pending {
                    Some(next) => app.set_lighting(next, cx),
                    None => {
                        app.system.lighting.shown = None;
                        // What the keyboard shows is what the daemon said.
                        if !matches!(app.system.lighting.reply, Some(Ok(_))) {
                            app.refresh_lighting(cx);
                        }
                    }
                }
                cx.notify();
            });
        }));
    }

    /// The card, after the keyboard backlight's.
    pub(crate) fn lighting_rows(
        &self,
        hint: &Card<LightingHint>,
        gap: f32,
        cx: &mut Context<Self>,
    ) -> Vec<Row> {
        let c = copy(self.locale);
        let header = c.header;
        let plain = |description: String, disabled: bool| {
            SettingsCard::new(card::LIGHTING, header)
                .icon(IconName::Palette)
                .description(description)
                .disabled(disabled)
        };
        let hint = match hint {
            Card::Hidden => return Vec::new(),
            Card::Disabled { .. } => {
                return vec![Row::card(gap, card::LIGHTING, plain(c.failed.into(), true))];
            }
            Card::Live(hint) => hint,
        };
        let view = lighting::view(hint, self.system.lighting.reply.as_ref());
        let live = match &view {
            LightingView::Live(live) => live.clone(),
            other => {
                let line = words::state_line(other, c).unwrap_or_default().to_string();
                let unavailable = matches!(other, LightingView::Unavailable(_));
                let mut card_ = plain(line, unavailable);
                if let LightingView::Unavailable(Access::Failed(details)) = other {
                    card_ = card_.caution(details.clone());
                }
                return vec![Row::card(gap, card::LIGHTING, card_)];
            }
        };
        vec![Row::card(
            gap,
            card::LIGHTING,
            self.lighting_expander(&live, cx),
        )]
    }

    fn lighting_expander(&self, live: &LiveLighting, cx: &mut Context<Self>) -> SettingsExpander {
        let c = copy(self.locale);
        let p = palette(cx);
        let current = self.system.lighting.shown.or(live.choice);
        let effect = current.map(|choice| choice.effect);
        let effects = live.effects;
        let items = effects
            .iter()
            .map(|e| ComboItem::new(words::effect_label(*e, c)))
            .collect();
        let selected = effect.and_then(|e| effects.iter().position(|o| *o == e));
        let mut description = words::live_description(live, current.as_ref(), c);
        if let Some(lamps) = live.lamps {
            description = format!("{description} · {}", words::zones(lamps, c));
        }
        let base = current.unwrap_or(LightingChoice::starting(Effect::Static));
        let mut expander =
            SettingsExpander::new(card::LIGHTING, c.header, self.system.lighting.open)
                .icon(IconName::Palette)
                .description(description)
                .content(
                    // A press on the list is the list's: it does not also
                    // open or close the expander under it.
                    div()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            ComboBox::new("system-lighting-effect", items, selected)
                                .min_width(px(SLIDER_WIDTH + 52.0))
                                .placeholder(c.choose_effect)
                                .on_select(Self::act_with(cx, move |this, index: usize, _, cx| {
                                    if let Some(effect) = effects.get(index).copied()
                                        && Some(effect) != this.lighting_effect()
                                    {
                                        this.system.lighting.open = effect.uses_colour()
                                            || effect.uses_speed()
                                            || this.system.lighting.open;
                                        this.set_lighting(LightingChoice { effect, ..base }, cx);
                                    }
                                })),
                        ),
                )
                .on_toggle(Self::act_with(cx, |this, open: bool, _, cx| {
                    this.system.lighting.open = open;
                    cx.notify();
                }));

        if let Some(choice) = current.filter(|c| c.effect.uses_colour()) {
            expander = expander.row(
                SettingsRow::new(c.colour)
                    .description(choice.colour.hex())
                    .content(self.swatches(choice, cx)),
            );
            let (hue, saturation) = choice.colour.hue_saturation();
            expander = expander.row(SettingsRow::new(c.hue).content(self.slider_with_value(
                "system-lighting-hue",
                hue.round(),
                SliderModel {
                    minimum: 0.0,
                    maximum: 359.0,
                    step: 1.0,
                    large_change: 30.0,
                },
                format!("{}°", hue.round() as u32),
                move |this, v, cx| {
                    let colour = with_hue(choice.colour, v);
                    if colour != this.lighting_colour() {
                        this.set_lighting(LightingChoice { colour, ..choice }, cx);
                    }
                },
                cx,
            )));
            expander = expander.row(SettingsRow::new(c.saturation).content(
                self.slider_with_value(
                    "system-lighting-saturation",
                    (saturation * 100.0).round(),
                    SliderModel::new(0.0, 100.0),
                    format!("{}%", (saturation * 100.0).round() as u32),
                    move |this, v, cx| {
                        let colour = with_saturation(choice.colour, v / 100.0);
                        if colour != this.lighting_colour() {
                            this.set_lighting(LightingChoice { colour, ..choice }, cx);
                        }
                    },
                    cx,
                ),
            ));
            if live.intensity {
                expander = expander.row(
                    SettingsRow::new(c.intensity)
                        .description(c.intensity_description)
                        .content(self.slider_with_value(
                            "system-lighting-intensity",
                            f64::from(choice.intensity),
                            SliderModel::new(0.0, 100.0),
                            format!("{}%", choice.intensity),
                            move |this, v, cx| {
                                let intensity = v.round().clamp(0.0, 100.0) as u8;
                                if Some(intensity) != this.lighting_current().map(|c| c.intensity) {
                                    this.set_lighting(
                                        LightingChoice {
                                            intensity,
                                            ..choice
                                        },
                                        cx,
                                    );
                                }
                            },
                            cx,
                        )),
                );
            }
        }
        if live.interface == Interface::AsusWmi
            && let Some(choice) = current.filter(|c| c.effect.uses_speed())
        {
            let options = Speed::ALL
                .iter()
                .map(|s| RadioOption::new(words::speed_label(*s, c)))
                .collect();
            let selected = Speed::ALL.iter().position(|s| *s == choice.speed);
            expander = expander.row(
                SettingsRow::new(c.speed).content(
                    RadioButtons::new("system-lighting-speed", options, selected)
                        .horizontal()
                        .on_select(Self::act_with(cx, move |this, index: usize, _, cx| {
                            if let Some(speed) = Speed::ALL.get(index).copied()
                                && Some(index) != selected
                            {
                                this.set_lighting(LightingChoice { speed, ..choice }, cx);
                            }
                        })),
                ),
            );
        }
        let note = super::decide::sentences(self.locale, &[words::note(live, c), c.at_startup]);
        expander.row(
            SettingsRow::new(c.about_effects).content(div().max_w(px(420.0)).child(label(
                note,
                BODY,
                p.text_secondary,
            ))),
        )
    }

    fn lighting_current(&self) -> Option<LightingChoice> {
        let stored = match &self.system.lighting.reply {
            Some(Ok(reply)) => reply.stored,
            _ => None,
        };
        self.system.lighting.shown.or(stored)
    }

    fn lighting_effect(&self) -> Option<Effect> {
        match self.lighting_current() {
            Some(choice) => Some(choice.effect),
            // A LampArray with nothing stored shows its own lighting.
            None => match &self.system.lighting.reply {
                Some(Ok(reply)) if reply.interface == Interface::LampArray => {
                    Some(Effect::Firmware)
                }
                _ => None,
            },
        }
    }

    fn lighting_colour(&self) -> Rgb {
        self.lighting_current()
            .map(|c| c.colour)
            .unwrap_or(Rgb::WHITE)
    }

    /// The colour picker's swatches, in the Fluent `ColorPicker` palette's
    /// shape: small rounded squares, the chosen one ringed in the accent.
    fn swatches(&self, choice: LightingChoice, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let mut row = div()
            .flex()
            .flex_wrap()
            .gap(px(6.0))
            .max_w(px(SLIDER_WIDTH + 52.0));
        for (index, swatch) in SWATCHES.iter().copied().enumerate() {
            let chosen = swatch == choice.colour;
            let on_click = Self::act(cx, move |this, _, cx| {
                this.set_lighting(
                    LightingChoice {
                        colour: swatch,
                        ..choice
                    },
                    cx,
                );
            });
            row = row.child(
                div()
                    .id(ElementId::named_usize("system-lighting-swatch", index))
                    .size(px(28.0))
                    .rounded(px(4.0))
                    .border_2()
                    .border_color(if chosen { p.accent } else { p.card })
                    .p(px(2.0))
                    .cursor_pointer()
                    .on_click(move |_, window, cx| on_click(window, cx))
                    .child(
                        div()
                            .size_full()
                            .rounded(px(2.0))
                            .border_1()
                            .border_color(p.card_stroke)
                            .bg(rgb(swatch.packed())),
                    ),
            );
        }
        row
    }

    #[allow(clippy::too_many_arguments)]
    fn slider_with_value(
        &self,
        id: &'static str,
        value: f64,
        model: SliderModel,
        shown: String,
        on_change: impl Fn(&mut Self, f64, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = palette(cx);
        div()
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(
                Slider::new(id, value, model)
                    .width(px(SLIDER_WIDTH))
                    .on_change(Self::act_with(cx, move |this, v: f64, _, cx| {
                        on_change(this, v, cx)
                    })),
            )
            .child(
                div()
                    .w(px(40.0))
                    .flex()
                    .justify_end()
                    .child(label(shown, BODY, p.text_primary)),
            )
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: that brings gpui's own `test` attribute in.
    use super::{with_hue, with_saturation};
    use hardware_core::lighting::Rgb;

    #[test]
    fn a_hue_drag_on_white_starts_from_full_saturation() {
        assert_eq!(with_hue(Rgb::WHITE, 120.0), Rgb::new(0, 255, 0));
        assert_eq!(with_hue(Rgb::new(255, 128, 128), 240.0).blue, 255);
        assert_eq!(with_saturation(Rgb::new(255, 0, 0), 0.0), Rgb::WHITE);
        assert_eq!(
            with_saturation(Rgb::new(255, 255, 255), 1.0),
            Rgb::new(255, 0, 0)
        );
    }
}

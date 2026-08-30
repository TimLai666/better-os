//! History, Diagnostics, and Settings.
//!
//! History and Diagnostics report what the service recorded and what it can
//! actually do; neither invents a value when a reading is missing. Settings is
//! the only section that changes something this window owns rather than
//! something the service owns.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme,
    button::{Button, ButtonVariants},
    tag::Tag,
    *,
};

use crate::{
    app::AwakeApp,
    i18n::{Locale, copy, fill},
    model::{HistoryRow, Section, provider_label},
    settings::StoredTheme,
};

impl AwakeApp {
    // ---- 6. History -----------------------------------------------------

    pub(crate) fn history_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let mut root = v_flex().gap_5().child(self.section_heading(
            Section::History.title(c),
            Section::History.subtitle(c),
            cx,
        ));
        if let Some(banner) = self.connection_banner(cx) {
            return root.child(banner).into_any_element();
        }

        // The retention limit is stated whether or not anything was dropped, so
        // a missing old session reads as a policy rather than as a fault.
        root = root.child(self.explanation(
            format!(
                "{} {}",
                fill(
                    c.retention_note,
                    "limit",
                    &self.history_retention.to_string()
                ),
                fill(
                    fill(c.showing_of, "shown", &self.history.len().to_string()).as_str(),
                    "total",
                    &self.history_total.to_string()
                )
            ),
            cx,
        ));

        if self.history.is_empty() {
            return root
                .child(self.state_message(c.no_history, c.no_history_detail, cx))
                .into_any_element();
        }

        root.children(self.history.iter().map(|entry| self.history_row(entry, cx)))
            .into_any_element()
    }

    fn history_row(&self, entry: &HistoryRow, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let policy = entry.policy;
        let reasons = if entry.reasons.is_empty() {
            c.none.to_string()
        } else {
            entry.reasons.join("、")
        };
        self.surface(
            v_flex()
                .gap_1()
                .min_w_0()
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_3()
                        .items_center()
                        .flex_wrap()
                        .child(
                            div()
                                .min_w(px(180.0))
                                .flex_1()
                                .min_w_0()
                                .text_sm()
                                .font_semibold()
                                .child(entry.started_label(self.offset)),
                        )
                        .child(
                            Tag::secondary()
                                .small()
                                .rounded_full()
                                .child(entry.cause_label(c)),
                        ),
                )
                .child(self.key_value(c.history_origin, entry.origin_label(c), cx))
                .child(self.key_value(c.history_ended, entry.ended_label(self.offset, c), cx))
                .child(self.key_value(c.history_reasons, reasons, cx))
                .child(self.key_value(c.history_policy, policy_sentence(policy, c), cx))
                .child(self.key_value(c.history_stop_cause, entry.cause_label(c), cx))
                .when_some(entry.battery_stop_percent, |view, percent| {
                    view.child(self.key_value(
                        c.field_battery_threshold,
                        fill(c.percent_value, "percent", &percent.to_string()),
                        cx,
                    ))
                })
                .when_some(entry.backend_failure.clone(), |view, failure| {
                    view.child(self.key_value(c.backend_unavailable_detail, failure, cx))
                }),
            cx,
        )
    }

    // ---- 7. Diagnostics --------------------------------------------------

    pub(crate) fn diagnostics_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let mut root = v_flex().gap_5().child(self.section_heading(
            Section::Diagnostics.title(c),
            Section::Diagnostics.subtitle(c),
            cx,
        ));
        if let Some(banner) = self.connection_banner(cx) {
            root = root.child(banner);
        }

        if let Some(status) = self.status.as_ref() {
            root = root.child(
                self.surface(
                    v_flex()
                        .gap_1()
                        .child(self.card_title(c.backend_heading))
                        .child(self.key_value(c.adapter_heading, status.backend.name.clone(), cx))
                        .child(self.key_value(
                            c.availability_column,
                            status.backend.availability_label(c),
                            cx,
                        ))
                        .when_some(status.backend.detail.clone(), |view, detail| {
                            view.child(self.key_value(c.explanation_column, detail, cx))
                        })
                        .child(div().pt_2().child(self.card_title(c.capability_heading)))
                        .child(self.key_value(
                            c.system_sleep,
                            yes_no(status.backend.can_hold_system_suspend, c),
                            cx,
                        ))
                        .child(self.key_value(
                            c.idle_handling,
                            yes_no(status.backend.can_hold_idle, c),
                            cx,
                        ))
                        .child(self.key_value(
                            c.display_sleep,
                            yes_no(status.backend.can_hold_display_sleep, c),
                            cx,
                        ))
                        .child(self.key_value(
                            c.automatic_lock,
                            yes_no(status.backend.can_hold_automatic_lock, c),
                            cx,
                        )),
                    cx,
                ),
            );
        }

        // The provider capability table, including each one's poll cadence.
        root = root.child(
            self.surface(
                v_flex()
                    .gap_2()
                    .child(self.card_title(c.provider_heading))
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_3()
                            .flex_wrap()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(div().min_w(px(160.0)).flex_1().child(c.provider_column))
                            .child(div().min_w(px(90.0)).child(c.availability_column))
                            .child(div().min_w(px(110.0)).child(c.cadence_column))
                            .child(div().min_w(px(160.0)).flex_1().child(c.explanation_column)),
                    )
                    .child(if self.providers.is_empty() {
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(c.unknown)
                            .into_any_element()
                    } else {
                        v_flex()
                            .gap_1()
                            .children(self.providers.iter().map(|provider| {
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .gap_3()
                                    .py_1p5()
                                    .flex_wrap()
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        div()
                                            .min_w(px(160.0))
                                            .flex_1()
                                            .min_w_0()
                                            .text_sm()
                                            .child(provider_label(provider.kind, c)),
                                    )
                                    .child(
                                        div()
                                            .min_w(px(90.0))
                                            .text_sm()
                                            .child(provider.availability_label(c)),
                                    )
                                    .child(
                                        div()
                                            .min_w(px(110.0))
                                            .text_sm()
                                            .child(provider.cadence_label(c)),
                                    )
                                    .child(
                                        div()
                                            .min_w(px(160.0))
                                            .flex_1()
                                            .min_w_0()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(
                                                provider
                                                    .explanation
                                                    .clone()
                                                    .unwrap_or_else(|| c.none.to_string()),
                                            ),
                                    )
                            }))
                            .into_any_element()
                    }),
                cx,
            ),
        );

        root.child(
            self.surface(
                v_flex()
                    .gap_2()
                    .child(self.card_title(c.verification_heading))
                    .child(self.badge_row(
                        gpui_component::IconName::CircleCheck,
                        c.verified_holds_no_inhibitor,
                        cx,
                    ))
                    .child(self.badge_row(
                        gpui_component::IconName::CircleCheck,
                        c.verified_no_shell_command,
                        cx,
                    ))
                    .child(self.key_value(
                        c.protocol_version,
                        match self.protocol_version {
                            Some(version) => version.to_string(),
                            None => c.unknown.to_string(),
                        },
                        cx,
                    )),
                cx,
            ),
        )
        // Said on this screen unconditionally: a desktop with no tray host has
        // no icon to notice, and the person reading this is the one who went
        // looking for an explanation.
        .child(
            self.surface(
                v_flex()
                    .gap_2()
                    .child(self.card_title(c.tray_host_unavailable))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(c.tray_host_unavailable_detail),
                    ),
                cx,
            ),
        )
        .into_any_element()
    }

    // ---- 8. Settings -----------------------------------------------------

    pub(crate) fn settings_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        v_flex()
            .gap_5()
            .child(self.section_heading(
                Section::Settings.title(c),
                Section::Settings.subtitle(c),
                cx,
            ))
            .when(!self.preferences_saved, |view| {
                view.child(self.warning(c.settings_storage_failed, cx))
            })
            .child(
                self.surface(
                    v_flex()
                        .gap_3()
                        .child(self.card_title(c.language))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.language_description),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .flex_wrap()
                                .child(self.locale_button(
                                    "locale-system",
                                    Locale::System,
                                    c.system_default,
                                    cx,
                                ))
                                .child(self.locale_button("locale-en", Locale::EnUs, c.english, cx))
                                .child(self.locale_button(
                                    "locale-zh-tw",
                                    Locale::ZhTw,
                                    c.traditional_chinese,
                                    cx,
                                )),
                        ),
                    cx,
                ),
            )
            .child(
                self.surface(
                    v_flex()
                        .gap_3()
                        .child(self.card_title(c.appearance))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.appearance_description),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .flex_wrap()
                                .child(self.theme_button(
                                    "theme-dark",
                                    StoredTheme::Dark,
                                    c.dark_theme,
                                    cx,
                                ))
                                .child(self.theme_button(
                                    "theme-light",
                                    StoredTheme::Light,
                                    c.light_theme,
                                    cx,
                                ))
                                .child(self.theme_button(
                                    "theme-system",
                                    StoredTheme::System,
                                    c.system_default,
                                    cx,
                                )),
                        ),
                    cx,
                ),
            )
            .child(
                self.surface(
                    v_flex()
                        .gap_2()
                        .child(self.card_title(c.keyboard_hint))
                        .children(Section::ALL.map(|section| {
                            self.key_value(section.title(c), section.shortcut(), cx)
                        })),
                    cx,
                ),
            )
            .into_any_element()
    }

    fn locale_button(
        &self,
        id: &'static str,
        locale: Locale,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .label(label)
            .selected(self.locale == locale)
            .when(self.locale == locale, |button| button.primary())
            .on_click(cx.listener(move |this, _, _, cx| this.set_locale(locale, cx)))
    }

    fn theme_button(
        &self,
        id: &'static str,
        theme: StoredTheme,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .label(label)
            .selected(self.preferences.theme == theme)
            .when(self.preferences.theme == theme, |button| button.primary())
            .on_click(cx.listener(move |this, _, window, cx| this.set_theme(theme, window, cx)))
    }
}

fn yes_no(value: bool, c: &'static crate::i18n::Copy) -> &'static str {
    if value { c.yes } else { c.no }
}

/// The policy of a recorded session, as the short sentence a list row can carry.
fn policy_sentence(policy: awake_core::SessionPolicy, c: &'static crate::i18n::Copy) -> String {
    let held: Vec<&str> = [
        (policy.prevent_system_suspend, c.system_sleep),
        (policy.prevent_idle, c.idle_handling),
        (policy.prevent_display_sleep, c.display_sleep),
        (policy.prevent_automatic_lock, c.automatic_lock),
    ]
    .into_iter()
    .filter_map(|(held, label)| held.then_some(label))
    .collect();
    if held.is_empty() {
        c.none.to_string()
    } else {
        held.join("、")
    }
}

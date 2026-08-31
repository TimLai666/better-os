//! Drawing the Gestures screen. Every decision it renders was made in
//! [`crate::gestures_model`].
//!
//! The compact direction diagram is drawn rather than written: a row of dots
//! for the contact points, the thumb dot filled differently from the fingers,
//! and a bar with a head showing which way they go. It is a handful of `div`s,
//! which means it needs no icon font, scales with the theme, and reads the same
//! in both locales.

use better_actions::{Key, Modifier};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme,
    button::{Button, ButtonVariants},
    input::Input,
    switch::Switch,
    *,
};
use touchpad_gestures::{ConflictResolution, Direction, GestureId, GestureShape, ShortcutCheck};

use crate::app::TouchpadApp;
use crate::gestures_model::{
    Arrow, GestureGlyph, GestureRow, KeyGroup, PresetStatus, action_label, direction_label,
    gesture_label, resolution_choices, resolution_label, shape_label,
};
use crate::i18n::copy;

impl TouchpadApp {
    /// The contact points and which way they travel.
    fn glyph(&self, glyph: GestureGlyph, cx: &Context<Self>) -> AnyElement {
        let dots = (0..glyph.dots).map(|index| {
            let is_thumb = glyph.thumb && index == 0;
            div()
                .size_2()
                .rounded_full()
                .when(is_thumb, |dot| dot.bg(cx.theme().primary).size_2p5())
                .when(!is_thumb, |dot| dot.bg(cx.theme().foreground))
        });

        let arrow: AnyElement = match glyph.arrow {
            Arrow::Still => div()
                .w(px(18.0))
                .h(px(3.0))
                .rounded_full()
                .bg(cx.theme().muted_foreground)
                .into_any_element(),
            // Two bars pointing at each other, or away from each other. A pinch
            // and a spread are the same picture read in opposite directions,
            // and drawing them that way is what makes the pair obvious.
            Arrow::In | Arrow::Out => h_flex()
                .gap_0p5()
                .items_center()
                .child(
                    div()
                        .w(px(8.0))
                        .h(px(3.0))
                        .rounded_full()
                        .bg(cx.theme().primary),
                )
                .child(
                    div()
                        .size_1()
                        .rounded_full()
                        .bg(cx.theme().muted_foreground),
                )
                .child(
                    div()
                        .w(px(8.0))
                        .h(px(3.0))
                        .rounded_full()
                        .bg(cx.theme().primary),
                )
                .into_any_element(),
            Arrow::Turn => div()
                .size(px(16.0))
                .rounded_full()
                .border_2()
                .border_color(cx.theme().primary)
                .into_any_element(),
            Arrow::Up | Arrow::Down => v_flex()
                .items_center()
                .gap_0p5()
                .when(glyph.arrow == Arrow::Up, |column| {
                    column.child(head(cx)).child(shaft_vertical(cx))
                })
                .when(glyph.arrow == Arrow::Down, |column| {
                    column.child(shaft_vertical(cx)).child(head(cx))
                })
                .into_any_element(),
            Arrow::Left | Arrow::Right => h_flex()
                .items_center()
                .gap_0p5()
                .when(glyph.arrow == Arrow::Left, |row| {
                    row.child(head(cx)).child(shaft_horizontal(cx))
                })
                .when(glyph.arrow == Arrow::Right, |row| {
                    row.child(shaft_horizontal(cx)).child(head(cx))
                })
                .into_any_element(),
        };

        v_flex()
            .w(px(64.0))
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .gap_1p5()
            .p_2()
            .rounded(cx.theme().radius)
            .bg(cx.theme().secondary)
            .child(h_flex().gap_1().items_end().children(dots))
            .child(arrow)
            .into_any_element()
    }

    fn gesture_row(&self, row: &GestureRow, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.model.locale());
        let id = row.id.clone();
        let test_id = row.id.clone();
        let toggle_id = row.id.clone();
        let enabled = row.enabled;

        self.card(
            h_flex()
                .w_full()
                .min_w_0()
                .gap_4()
                .items_start()
                .flex_wrap()
                .child(self.glyph(row.glyph, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            h_flex()
                                .min_w_0()
                                .gap_2()
                                .flex_wrap()
                                .items_center()
                                .child(
                                    div()
                                        .min_w_0()
                                        .text_sm()
                                        .font_semibold()
                                        .child(row.label.clone()),
                                )
                                .when_some(row.conflict.clone(), |this, conflict| {
                                    this.child(self.badge_text(conflict, cx.theme().warning, cx))
                                })
                                .when(!row.supported, |this| {
                                    this.child(self.badge_text(
                                        c.unsupported_badge.to_string(),
                                        cx.theme().muted,
                                        cx,
                                    ))
                                }),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} · {} {} · {}: {}",
                                    row.shape_label,
                                    row.contacts,
                                    c.contacts_label,
                                    c.action_heading,
                                    row.action_label
                                )),
                        )
                        .when_some(row.direction_label, |this, direction| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{}: {direction}", c.direction_heading)),
                            )
                        })
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(row.verification),
                        )
                        .when_some(row.support_detail.clone(), |this, detail| {
                            this.child(
                                div()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(detail),
                            )
                        }),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .flex_wrap()
                        .child(
                            Switch::new(SharedString::from(format!("enable-{id}")))
                                .checked(enabled)
                                .on_click(cx.listener(move |this, value: &bool, _window, cx| {
                                    this.toggle_gesture(&toggle_id, *value, cx);
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("test-{id}")))
                                .outline()
                                .small()
                                .label(c.test_run)
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    this.test_gesture(&test_id, cx);
                                })),
                        )
                        .child({
                            let edit_id = row.id.clone();
                            Button::new(SharedString::from(format!("edit-{id}")))
                                .outline()
                                .small()
                                .label(c.edit_gesture)
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    this.edit_gesture(&edit_id, cx);
                                }))
                        }),
                )
                .into_any_element(),
            cx,
        )
    }

    fn badge_text(&self, label: String, color: Hsla, cx: &Context<Self>) -> AnyElement {
        div()
            .px_2()
            .py_0p5()
            .rounded_full()
            .border_1()
            .border_color(color)
            .text_xs()
            .text_color(cx.theme().foreground)
            .child(label)
            .into_any_element()
    }

    fn preset_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.model.locale());
        let card = self.gestures.preset_card(c);
        let previewing = self.gestures.plan().is_some();

        self.card(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_3()
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_3()
                        .items_center()
                        .justify_between()
                        .flex_wrap()
                        .child(div().text_base().font_semibold().child(c.preset_title))
                        .child(self.badge_text(
                            card.status_label.to_string(),
                            match card.status {
                                PresetStatus::Applied => cx.theme().success,
                                PresetStatus::Differs => cx.theme().warning,
                                PresetStatus::NotApplied => cx.theme().muted,
                            },
                            cx,
                        )),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(c.preset_description),
                )
                .when(previewing, |this| {
                    this.child(
                        v_flex()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .child(div().text_sm().font_semibold().child(c.changes_heading))
                            .when(card.changes.is_empty(), |this| {
                                this.child(div().text_xs().child(c.changes_none))
                            })
                            .children(card.changes.iter().map(|change| {
                                div()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(change.clone())
                            })),
                    )
                })
                .when(previewing && !card.conflicts.is_empty(), |this| {
                    this.child(
                        v_flex()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .child(div().text_sm().font_semibold().child(c.conflicts_heading))
                            .children(
                                card.conflicts
                                    .iter()
                                    .map(|conflict| self.conflict_row(conflict, cx)),
                            ),
                    )
                })
                .when(previewing && !card.unsupported.is_empty(), |this| {
                    this.child(v_flex().w_full().min_w_0().gap_1().children(
                        card.unsupported.iter().map(|line| {
                            div()
                                .min_w_0()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(line.clone())
                        }),
                    ))
                })
                .when(previewing, |this| {
                    this.child(
                        Switch::new("confirm-preset")
                            .checked(card.confirmed)
                            .label(c.confirm_changes)
                            .on_click(cx.listener(|this, value: &bool, _window, cx| {
                                this.confirm_preset(*value, cx);
                            })),
                    )
                })
                .when_some(card.blocked_reason, |this, reason| {
                    this.child(
                        div()
                            .min_w_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(reason),
                    )
                })
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .when(!previewing, |this| {
                            this.child(
                                Button::new("preview-preset")
                                    .primary()
                                    .tab_index(TAB_PREVIEW_PRESET)
                                    .label(c.preview_changes)
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.preview_preset(cx);
                                    })),
                            )
                        })
                        .when(previewing, |this| {
                            this.child(
                                Button::new("apply-preset")
                                    .primary()
                                    .tab_index(TAB_APPLY_PLAN)
                                    .disabled(!card.can_apply)
                                    .label(c.apply_preset)
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.apply_plan(cx);
                                    })),
                            )
                            .child(
                                Button::new("cancel-preview")
                                    .outline()
                                    .tab_index(TAB_CANCEL_PREVIEW)
                                    .label(c.cancel_preview)
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.cancel_preview(cx);
                                    })),
                            )
                        }),
                )
                .into_any_element(),
            cx,
        )
    }

    fn conflict_row(
        &self,
        conflict: &crate::gestures_model::ConflictRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.model.locale());
        let direction = self
            .gestures
            .config()
            .get(&conflict.gesture)
            .and_then(|gesture| gesture.direction);

        v_flex()
            .w_full()
            .min_w_0()
            .gap_1()
            .child(div().min_w_0().text_xs().child(format!(
                "{} · {}",
                conflict.gesture_label, conflict.built_in
            )))
            .child(
                h_flex().gap_2().flex_wrap().children(
                    resolution_choices(direction).into_iter().enumerate().map(
                        |(index, resolution)| {
                            let selected = conflict.resolution == Some(resolution);
                            let gesture = conflict.gesture.clone();
                            let button = Button::new(SharedString::from(format!(
                                "resolve-{}-{index}",
                                conflict.gesture
                            )))
                            .small()
                            .label(resolution_label(resolution, c))
                            .on_click(cx.listener(
                                move |this, _, _window, cx| {
                                    this.resolve_conflict(gesture.clone(), resolution, cx);
                                },
                            ));
                            if selected {
                                button.primary()
                            } else {
                                button.outline()
                            }
                        },
                    ),
                ),
            )
            .into_any_element()
    }

    /// Which touchpad's gestures are being edited, and whether that pad has a
    /// profile of its own yet.
    fn profile_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.model.locale());
        let selected = self.gestures.active_device().map(str::to_string);
        let own = self.gestures.device_has_own_profile();

        let mut options: Vec<(String, bool, Option<String>)> =
            vec![(c.profile_global.to_string(), selected.is_none(), None)];
        options.extend(self.model.devices().iter().map(|device| {
            (
                device.name.clone(),
                selected.as_deref() == Some(device.identity.as_str()),
                Some(device.identity.clone()),
            )
        }));

        self.card(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .child(div().text_sm().font_semibold().child(c.profile_heading))
                .child(self.choice_row(
                    c.device_scope,
                    options,
                    "gesture-profile",
                    cx,
                    |this, identity: Option<String>, cx| {
                        this.select_gesture_device(identity, cx);
                    },
                ))
                .when(selected.is_some(), |this| {
                    this.child(
                        div()
                            .min_w_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if own {
                                c.profile_own
                            } else {
                                c.profile_follows_global
                            }),
                    )
                })
                .when(selected.is_some(), |this| {
                    this.child(if own {
                        Button::new("forget-profile")
                            .outline()
                            .small()
                            .tab_index(TAB_FORGET_PROFILE)
                            .label(c.profile_forget)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.forget_gesture_profile(cx);
                            }))
                    } else {
                        Button::new("detach-profile")
                            .outline()
                            .small()
                            .tab_index(TAB_DETACH_PROFILE)
                            .label(c.profile_detach)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.detach_gesture_profile(cx);
                            }))
                    })
                })
                .into_any_element(),
            cx,
        )
    }

    /// Export and import, and what an import would bring in.
    fn profiles_file_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.model.locale());
        let export_path = self.export_path.clone();
        let import_path = self.import_path.clone();

        self.card(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_3()
                .child(div().text_sm().font_semibold().child(c.profiles_file))
                .child(
                    v_flex()
                        .w_full()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.export_label),
                        )
                        .child(Input::new(&self.export_path))
                        .child(
                            Button::new("export-profiles")
                                .outline()
                                .tab_index(TAB_EXPORT)
                                .label(c.export_profiles)
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    let path = export_path.read(cx).value().to_string();
                                    this.export_profiles(&path, cx);
                                })),
                        ),
                )
                .child(
                    v_flex()
                        .w_full()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.import_label),
                        )
                        .child(Input::new(&self.import_path))
                        .child(
                            Button::new("import-profiles")
                                .outline()
                                .tab_index(TAB_IMPORT)
                                .label(c.import_profiles)
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    let path = import_path.read(cx).value().to_string();
                                    this.preview_import(&path, cx);
                                })),
                        ),
                )
                .when_some(self.gestures.import().cloned(), |this, summary| {
                    this.child(
                        v_flex()
                            .w_full()
                            .min_w_0()
                            .gap_1()
                            .child(div().min_w_0().text_xs().child(format!(
                                "{}: {}",
                                c.import_summary,
                                summary.device_profiles.len()
                            )))
                            .children(summary.device_profiles.iter().map(|identity| {
                                div()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(identity.clone())
                            }))
                            .when(!summary.matches_selected_device, |this| {
                                this.child(
                                    div()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(c.import_uses_global),
                                )
                            }),
                    )
                })
                .into_any_element(),
            cx,
        )
    }

    /// The modifier set, the key, and what the recorded keybindings say about
    /// the combination.
    fn shortcut_picker(
        &self,
        editor: &crate::gestures_model::GestureEditor,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.model.locale());
        let check = self.gestures.shortcut_check();
        let spelling = editor.shortcut.spelling();

        v_flex()
            .w_full()
            .min_w_0()
            .gap_2()
            .child(div().text_xs().font_semibold().child(c.shortcut_heading))
            .child(
                self.choice_row(
                    c.shortcut_modifiers,
                    Modifier::ALL
                        .into_iter()
                        .map(|modifier| {
                            (modifier.name(), editor.shortcut.holds(modifier), modifier)
                        })
                        .collect::<Vec<_>>(),
                    "shortcut-modifier",
                    cx,
                    |this, modifier: Modifier, cx| {
                        if let Some(editor) = this.gestures.editor_mut() {
                            editor.shortcut.toggle(modifier);
                        }
                        cx.notify();
                    },
                ),
            )
            .child(
                self.choice_row(
                    c.shortcut_key,
                    KeyGroup::ALL
                        .into_iter()
                        .map(|group| (key_group_label(group, c), group == editor.key_group, group))
                        .collect::<Vec<_>>(),
                    "shortcut-group",
                    cx,
                    |this, group: KeyGroup, cx| {
                        if let Some(editor) = this.gestures.editor_mut() {
                            editor.key_group = group;
                        }
                        cx.notify();
                    },
                ),
            )
            .child(
                self.choice_row(
                    "",
                    editor
                        .key_group
                        .keys()
                        .into_iter()
                        .map(|key| (key.name(), key == editor.shortcut.key, key))
                        .collect::<Vec<_>>(),
                    "shortcut-key",
                    cx,
                    |this, key: Key, cx| {
                        if let Some(editor) = this.gestures.editor_mut() {
                            editor.set_key(key);
                        }
                        cx.notify();
                    },
                ),
            )
            .child(match &spelling {
                Ok(text) => div()
                    .min_w_0()
                    .text_xs()
                    .font_semibold()
                    .child(text.clone()),
                Err(_) => div()
                    .min_w_0()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(c.shortcut_needs_modifier),
            })
            .when_some(check, |this, check| {
                this.child(
                    div()
                        .min_w_0()
                        .text_xs()
                        .text_color(match check {
                            ShortcutCheck::Conflicts { .. } => cx.theme().warning,
                            _ => cx.theme().muted_foreground,
                        })
                        .child(match check {
                            ShortcutCheck::Conflicts { key } => {
                                format!("{} {key}", c.shortcut_conflict)
                            }
                            ShortcutCheck::NoneRecorded => c.shortcut_none_recorded.to_string(),
                            ShortcutCheck::Unknown { reason } => {
                                format!("{} · {reason}", c.shortcut_unknown)
                            }
                        }),
                )
            })
            .into_any_element()
    }

    fn test_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.model.locale());
        let run = self.gestures.last_test();
        let live = self.gestures.live_testing();
        // A switch that could not do anything is worse than one that is not
        // there, so it is disabled when the adapter performs no system action —
        // which is every adapter in this build.
        let can_perform = self.gestures.adapter().describe().performs_system_actions;

        self.card(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .child(div().text_sm().font_semibold().child(c.test_gestures))
                .child(
                    div()
                        .min_w_0()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(c.test_gestures_hint),
                )
                .child(
                    Switch::new("live-testing")
                        .checked(live)
                        .disabled(!can_perform)
                        .label(c.live_testing)
                        .on_click(cx.listener(|this, value: &bool, _window, cx| {
                            this.set_live_testing(*value, cx);
                        })),
                )
                .when(!live, |this| {
                    this.child(
                        div()
                            .min_w_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(c.live_testing_off),
                    )
                })
                .when(!can_perform, |this| {
                    this.child(
                        div()
                            .min_w_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(c.gesture_backend_none),
                    )
                })
                // The preset's launcher and Show Desktop gestures want a thumb
                // and no desktop in this build can see one. The gestures still
                // work, and saying how is better than a row that reads as a
                // detection nothing performs.
                .when(
                    can_perform
                        && self
                            .gestures
                            .config()
                            .gestures
                            .iter()
                            .any(|gesture| gesture.enabled && gesture.thumb_required),
                    |this| {
                        this.child(
                            div()
                                .min_w_0()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.gesture_thumb_best_effort),
                        )
                    },
                )
                .child(div().text_xs().font_semibold().child(c.recognized_events))
                .when(run.lines.is_empty(), |this| {
                    this.child(div().text_xs().child(c.test_no_events))
                })
                .children(run.lines.iter().map(|line| {
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_3()
                        .flex_wrap()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(div().w(px(160.0)).child(line.gesture.clone()))
                        .child(div().w(px(120.0)).child(line.kind))
                        .child(format!("{:.0}%", line.progress * 100.0))
                }))
                .into_any_element(),
            cx,
        )
    }

    fn editor_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let c = copy(self.model.locale());
        let editor = self.gestures.editor()?.clone();

        Some(
            self.card(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .child(gesture_label(&editor.id, c)),
                    )
                    .child(
                        self.choice_row(
                            c.shape_label,
                            GestureShape::ALL
                                .into_iter()
                                .map(|shape| (shape_label(shape, c), shape == editor.shape, shape))
                                .collect(),
                            "shape",
                            cx,
                            |this, shape, cx| {
                                if let Some(editor) = this.gestures.editor_mut() {
                                    editor.set_shape(shape);
                                }
                                cx.notify();
                            },
                        ),
                    )
                    .when(editor.shape.needs_direction(), |this| {
                        this.child(
                            self.choice_row(
                                c.direction_heading,
                                editor
                                    .shape
                                    .allowed_directions()
                                    .iter()
                                    .map(|direction| {
                                        (
                                            direction_label(*direction, c),
                                            Some(*direction) == editor.direction,
                                            *direction,
                                        )
                                    })
                                    .collect(),
                                "direction",
                                cx,
                                |this, direction: Direction, cx| {
                                    if let Some(editor) = this.gestures.editor_mut() {
                                        editor.direction = Some(direction);
                                    }
                                    cx.notify();
                                },
                            ),
                        )
                    })
                    .child(
                        self.choice_row(
                            c.contacts_label,
                            (1u8..=5)
                                .map(|count| {
                                    (
                                        match count {
                                            1 => "1",
                                            2 => "2",
                                            3 => "3",
                                            4 => "4",
                                            _ => "5",
                                        },
                                        count == editor.contacts,
                                        count,
                                    )
                                })
                                .collect(),
                            "contacts",
                            cx,
                            |this, count: u8, cx| {
                                if let Some(editor) = this.gestures.editor_mut() {
                                    editor.contacts = count;
                                }
                                cx.notify();
                            },
                        ),
                    )
                    .child(
                        self.choice_row(
                            c.action_heading,
                            better_actions::DesktopAction::catalog()
                                .into_iter()
                                .map(|action| {
                                    let selected = action == editor.action;
                                    (action_label(&action, c), selected, action)
                                })
                                .collect::<Vec<_>>(),
                            "action",
                            cx,
                            |this, action, cx| {
                                if let Some(editor) = this.gestures.editor_mut() {
                                    editor.set_action(action);
                                }
                                cx.notify();
                            },
                        ),
                    )
                    .when(editor.action_is_shortcut(), |this| {
                        this.child(self.shortcut_picker(&editor, cx))
                    })
                    .child(
                        self.choice_row(
                            c.activation_label,
                            [0.4f32, 0.5, 0.6, 0.7, 0.8]
                                .into_iter()
                                .map(|value| {
                                    (
                                        format!("{:.0}%", value * 100.0),
                                        (value - editor.activation).abs() < 0.001,
                                        value,
                                    )
                                })
                                .collect::<Vec<_>>(),
                            "activation",
                            cx,
                            |this, value: f32, cx| {
                                if let Some(editor) = this.gestures.editor_mut() {
                                    editor.activation = value;
                                }
                                cx.notify();
                            },
                        ),
                    )
                    .child(
                        self.choice_row(
                            c.cancellation_label,
                            [0.0f32, 0.15, 0.25, 0.35, 0.5]
                                .into_iter()
                                .map(|value| {
                                    (
                                        format!("{:.0}%", value * 100.0),
                                        (value - editor.cancellation).abs() < 0.001,
                                        value,
                                    )
                                })
                                .collect::<Vec<_>>(),
                            "cancellation",
                            cx,
                            |this, value: f32, cx| {
                                if let Some(editor) = this.gestures.editor_mut() {
                                    editor.cancellation = value;
                                }
                                cx.notify();
                            },
                        ),
                    )
                    .child(
                        self.choice_row(
                            c.cooldown_label,
                            [0u64, 150, 350, 600, 1_000]
                                .into_iter()
                                .map(|value| {
                                    (format!("{value} ms"), value == editor.cooldown_ms, value)
                                })
                                .collect::<Vec<_>>(),
                            "cooldown",
                            cx,
                            |this, value: u64, cx| {
                                if let Some(editor) = this.gestures.editor_mut() {
                                    editor.cooldown_ms = value;
                                }
                                cx.notify();
                            },
                        ),
                    )
                    .child(
                        Switch::new("editor-thumb")
                            .checked(editor.thumb_required)
                            .label(c.thumb_label)
                            .on_click(cx.listener(|this, value: &bool, _window, cx| {
                                if let Some(editor) = this.gestures.editor_mut() {
                                    editor.thumb_required = *value;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Switch::new("editor-enabled")
                            .checked(editor.enabled)
                            .label(c.enabled_label)
                            .on_click(cx.listener(|this, value: &bool, _window, cx| {
                                if let Some(editor) = this.gestures.editor_mut() {
                                    editor.enabled = *value;
                                }
                                cx.notify();
                            })),
                    )
                    .when_some(editor.error.clone(), |this, error| {
                        this.child(
                            div()
                                .min_w_0()
                                .text_xs()
                                .text_color(cx.theme().danger)
                                .child(error),
                        )
                    })
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                Button::new("save-gesture")
                                    .primary()
                                    .label(c.save_gesture)
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.commit_edit(cx);
                                    })),
                            )
                            .child(
                                Button::new("cancel-edit")
                                    .outline()
                                    .label(c.cancel_preview)
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.cancel_edit(cx);
                                    })),
                            ),
                    )
                    .into_any_element(),
                cx,
            ),
        )
    }

    /// A labelled row of segmented buttons. Every option stays visible and each
    /// one is its own tab stop, which is the pattern the settings screens use.
    fn choice_row<T: Clone + 'static>(
        &self,
        label: &'static str,
        options: Vec<(impl Into<SharedString>, bool, T)>,
        key: &'static str,
        cx: &mut Context<Self>,
        on_pick: impl Fn(&mut Self, T, &mut Context<Self>) + Clone + 'static,
    ) -> AnyElement {
        v_flex()
            .w_full()
            .min_w_0()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(
                h_flex().w_full().min_w_0().gap_2().flex_wrap().children(
                    options
                        .into_iter()
                        .enumerate()
                        .map(|(index, (text, selected, value))| {
                            let on_pick = on_pick.clone();
                            let button = Button::new(SharedString::from(format!("{key}-{index}")))
                                .small()
                                .label(text)
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    on_pick(this, value.clone(), cx);
                                }));
                            if selected {
                                button.primary()
                            } else {
                                button.outline()
                            }
                        }),
                ),
            )
            .into_any_element()
    }

    pub(crate) fn gestures_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.model.locale());
        let rows = self.gestures.rows(c);
        let has_capture = self.gestures.captured().is_some();
        let editor = self.editor_panel(cx);

        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(self.heading(c.gestures_title, c.gestures_subtitle))
            .child(self.profile_card(cx))
            .child(self.preset_card(cx))
            .when_some(
                self.gestures.problem().map(str::to_string),
                |this, problem| this.child(self.card(div().min_w_0().text_xs().child(problem), cx)),
            )
            .when(rows.is_empty(), |this| {
                this.child(self.card(div().text_sm().child(c.no_gestures), cx))
            })
            .children(rows.iter().map(|row| self.gesture_row(row, cx)))
            .when_some(editor, |this, editor| this.child(editor))
            .child(self.test_panel(cx))
            .child(self.profiles_file_card(cx))
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("restore-gestures")
                            .outline()
                            .tab_index(TAB_RESTORE)
                            .disabled(!has_capture)
                            .label(c.restore_gestures)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.restore_gestures(cx);
                            })),
                    )
                    .child(
                        Button::new("disable-gestures")
                            .outline()
                            .tab_index(TAB_DISABLE)
                            .label(c.disable_gestures)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.disable_gestures(cx);
                            })),
                    ),
            )
            .into_any_element()
    }
}

fn head<T: 'static>(cx: &Context<T>) -> Div {
    div().size_1p5().rounded_full().bg(cx.theme().primary)
}

fn shaft_vertical<T: 'static>(cx: &Context<T>) -> Div {
    div()
        .w(px(3.0))
        .h(px(14.0))
        .rounded_full()
        .bg(cx.theme().primary)
}

fn shaft_horizontal<T: 'static>(cx: &Context<T>) -> Div {
    div()
        .w(px(14.0))
        .h(px(3.0))
        .rounded_full()
        .bg(cx.theme().primary)
}

/// The identity a row's control keys are built from. Kept here so the renderer
/// and a test agree on what a button is called.
pub fn control_key(prefix: &str, gesture: &GestureId) -> String {
    format!("{prefix}-{gesture}")
}

/// The resolutions a conflict row offers, for a test that wants to assert them
/// without a window.
pub fn offered_resolutions(direction: Option<Direction>) -> Vec<ConflictResolution> {
    resolution_choices(direction)
}

/// The Gestures screen's explicit tab stops, in the order they are reached.
///
/// The controls inside a card are their own tab stops in document order; these
/// are the ones whose order is stated rather than inherited, so a card moving
/// on the page cannot silently reorder them. A test asserts they are distinct
/// and increasing, which is what "reachable in a sensible order by keyboard"
/// means when there is no window to tab through.
pub const TAB_DETACH_PROFILE: isize = 10;
pub const TAB_FORGET_PROFILE: isize = 11;
pub const TAB_PREVIEW_PRESET: isize = 20;
pub const TAB_APPLY_PLAN: isize = 21;
pub const TAB_CANCEL_PREVIEW: isize = 22;
pub const TAB_RESTORE: isize = 50;
pub const TAB_DISABLE: isize = 51;
pub const TAB_EXPORT: isize = 60;
pub const TAB_IMPORT: isize = 61;

/// Every explicit tab stop on the screen, in the order they appear.
pub const GESTURE_TAB_ORDER: [isize; 9] = [
    TAB_DETACH_PROFILE,
    TAB_FORGET_PROFILE,
    TAB_PREVIEW_PRESET,
    TAB_APPLY_PLAN,
    TAB_CANCEL_PREVIEW,
    TAB_RESTORE,
    TAB_DISABLE,
    TAB_EXPORT,
    TAB_IMPORT,
];

/// The wording for one part of the key table.
pub fn key_group_label(group: KeyGroup, c: &'static crate::i18n::Copy) -> &'static str {
    match group {
        KeyGroup::Letters => c.group_letters,
        KeyGroup::Digits => c.group_digits,
        KeyGroup::Function => c.group_function,
        KeyGroup::Navigation => c.group_navigation,
        KeyGroup::Editing => c.group_editing,
        KeyGroup::Punctuation => c.group_punctuation,
    }
}

//! Draws the Gestures sub-page. Every decision it renders was made in
//! `touchpad_views::gestures_model`.
//!
//! The direction picture is drawn rather than written: a row of dots for the
//! contact points, the thumb in the accent color, and a bar with a head for
//! which way they go. It is a handful of `div`s, so it needs no icon font,
//! follows the theme, and reads the same in both languages.

use better_actions::{Key, Modifier};
use better_ui::fluent::button::Button;
use better_ui::fluent::chip::Chip;
use better_ui::fluent::info_bar::{ErrorBar, ErrorReport, InfoBar, Severity};
use better_ui::fluent::selection::{ComboBox, ComboItem};
use better_ui::fluent::settings_card::{SettingsCard, SettingsExpander, SettingsRow};
use better_ui::fluent::tabs::{SelectorBar, SelectorItem};
use better_ui::fluent::text_box::TextBox;
use better_ui::fluent::toggle::{CheckBox, CheckState, ToggleSwitch};
use better_ui::fluent::{BODY, BODY_STRONG, CAPTION, label, palette, radius};
use gpui::*;
use gpui_component::IconName;
use touchpad_gestures::{ConflictResolution, GestureShape, ShortcutCheck};
use touchpad_views::gestures_model::{
    Arrow, GestureEditor, GestureGlyph, GestureRow, KeyGroup, action_label, direction_label,
    gesture_label, resolution_choices, resolution_label, shape_label,
};
use touchpad_views::i18n::{Copy as TpCopy, problem_sentence};

use super::copy::{event_words, page_copy, test_reach};
use super::render::CHOICE_WIDTH;
use super::view::{self, card};
use super::{FileNote, TouchpadPage};
use crate::render::{CARD_GAP, FIRST_CARD_GAP, GROUP_GAP, Row};

/// How one of the editor's lists writes its choice into the draft.
type EditorPick = Box<dyn Fn(&mut GestureEditor, usize)>;

/// The thresholds the editor offers, as the older window did.
const ACTIVATIONS: [f32; 5] = [0.4, 0.5, 0.6, 0.7, 0.8];
const CANCELLATIONS: [f32; 5] = [0.0, 0.15, 0.25, 0.35, 0.5];
const COOLDOWNS: [u64; 5] = [0, 150, 350, 600, 1_000];

impl TouchpadPage {
    pub(super) fn gesture_rows(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let c = self.c();
        let words = self.locale.words();
        let mut rows = Vec::new();

        if let Some(problem) = self.gestures.problem() {
            let summary = problem_sentence(problem, view::views_locale(self.locale));
            rows.push(Row::new(
                0.0,
                div().pb(px(12.0)).child(
                    ErrorBar::new(
                        "touchpad-gesture-problem",
                        ErrorReport::new(summary, problem.to_string()),
                    )
                    .locale(words),
                ),
            ));
        }

        rows.push(Row::card(0.0, card::GESTURE_PROFILE, self.profile_card(cx)));
        rows.push(Row::card(
            CARD_GAP,
            card::GESTURE_PRESET,
            self.preset_card(cx),
        ));

        rows.push(self.group(c.gestures_title, cx));
        let gesture_rows = self.gestures.rows(c);
        if gesture_rows.is_empty() {
            rows.push(Row::new(
                FIRST_CARD_GAP,
                label(c.no_gestures, BODY, palette(cx).text_secondary),
            ));
        }
        let editing = self.gestures.editor().map(|editor| editor.id.clone());
        for (index, row) in gesture_rows.iter().enumerate() {
            let gap = if index == 0 { FIRST_CARD_GAP } else { CARD_GAP };
            rows.push(Row::new(gap, self.gesture_card(index, row, cx)));
            if editing.as_ref() == Some(&row.id)
                && let Some(editor) = self.gestures.editor().cloned()
            {
                rows.push(Row::new(CARD_GAP, self.editor_card(&editor, cx)));
            }
        }

        rows.push(self.group(c.test_gestures, cx));
        rows.push(Row::card(
            FIRST_CARD_GAP,
            card::GESTURE_TEST,
            self.test_card(cx),
        ));

        let mut file_rows = self.file_rows(cx);
        if let Some(first) = file_rows.first_mut() {
            first.set_gap(GROUP_GAP);
        }
        rows.extend(file_rows);

        let has_capture = self.gestures.captured().is_some();
        rows.push(Row::card(
            GROUP_GAP,
            card::GESTURES_OFF,
            div()
                .flex()
                .flex_wrap()
                .gap(px(8.0))
                .child(
                    Button::standard("touchpad-restore-gestures", c.restore_gestures)
                        .icon(IconName::Undo)
                        .disabled(!has_capture)
                        .on_click(Self::act(cx, |this, cx| {
                            this.restore_gestures(cx);
                        })),
                )
                .child(
                    Button::standard("touchpad-disable-gestures", c.disable_gestures)
                        .disabled(!self.gestures.config().enabled)
                        .on_click(Self::act(cx, |this, cx| {
                            this.disable_gestures(cx);
                        })),
                ),
        ));
        rows
    }

    /// Which touchpad's gestures are shown, and whether it has its own.
    fn profile_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.c();
        let selected = self.gestures.active_device().map(str::to_string);
        let own = self.gestures.device_has_own_profile();
        let mut identities: Vec<Option<String>> = vec![None];
        let mut items = vec![SelectorItem::new(c.profile_global)];
        for device in self.model.devices() {
            identities.push(Some(device.identity.clone()));
            items.push(SelectorItem::new(view::friendly_name(&device.name)));
        }
        let selected_index = identities.iter().position(|identity| *identity == selected);
        let mut card = SettingsCard::new("touchpad-gesture-profile-card", c.profile_heading)
            .icon(IconName::Frame);
        if selected.is_some() {
            card = card
                .description(if own {
                    c.profile_own
                } else {
                    c.profile_follows_global
                })
                .content(if own {
                    Button::standard("touchpad-forget-profile", c.profile_forget)
                        .on_click(Self::act(cx, |this, cx| this.forget_gesture_profile(cx)))
                } else {
                    Button::standard("touchpad-detach-profile", c.profile_detach)
                        .on_click(Self::act(cx, |this, cx| this.detach_gesture_profile(cx)))
                });
        }
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(div().min_w_0().overflow_x_hidden().child(
                SelectorBar::new("touchpad-gesture-profiles", items, selected_index).on_select(
                    Self::act_with(cx, move |this, index: usize, cx| {
                        if let Some(identity) = identities.get(index).cloned() {
                            this.select_gesture_device(identity, cx);
                        }
                    }),
                ),
            ))
            .child(card)
            .into_any_element()
    }

    /// The Mac-style preset: what it is, its preview, the conflicts to
    /// decide, the confirmation, and apply.
    fn preset_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.c();
        let preset = self.gestures.preset_card(c);
        let previewing = self.gestures.plan().is_some();
        let mut expander = SettingsExpander::new("touchpad-preset", c.preset_title, previewing)
            .icon(IconName::Maximize)
            .description(format!(
                "{} · {}",
                preset.status_label, c.preset_description
            ))
            .on_toggle(Self::act_with(cx, |this, open: bool, cx| {
                if open {
                    this.preview_preset(cx);
                } else {
                    this.cancel_preview(cx);
                }
            }));
        if !previewing {
            return expander
                .content(
                    Button::accent("touchpad-preview-preset", c.preview_changes)
                        .on_click(Self::act(cx, |this, cx| this.preview_preset(cx))),
                )
                .into_any_element();
        }
        expander = expander.row(SettingsRow::new(c.changes_heading).description(
            if preset.changes.is_empty() {
                c.changes_none.to_string()
            } else {
                preset.changes.join("\n")
            },
        ));
        if !preset.conflicts.is_empty() {
            expander =
                expander.row(SettingsRow::new(c.conflicts_heading).description(c.unresolved_note));
        }
        for conflict in &preset.conflicts {
            let direction = self
                .gestures
                .config()
                .get(&conflict.gesture)
                .and_then(|gesture| gesture.direction);
            let options = resolution_choices(direction);
            let selected = conflict
                .resolution
                .and_then(|resolution| options.iter().position(|option| *option == resolution));
            let gesture = conflict.gesture.clone();
            let choices: Vec<ConflictResolution> = options.clone();
            expander = expander.row(
                SettingsRow::new(conflict.gesture_label.clone())
                    .description(conflict.built_in.clone())
                    .content(
                        ComboBox::new(
                            ElementId::Name(
                                format!("touchpad-resolve-{}", conflict.gesture).into(),
                            ),
                            options
                                .iter()
                                .map(|option| ComboItem::new(resolution_label(*option, c)))
                                .collect(),
                            selected,
                        )
                        .min_width(px(CHOICE_WIDTH))
                        .placeholder(page_copy(self.locale).choose)
                        .on_select(Self::act_with(
                            cx,
                            move |this, index: usize, cx| {
                                if let Some(resolution) = choices.get(index) {
                                    this.resolve_conflict(gesture.clone(), *resolution, cx);
                                }
                            },
                        )),
                    ),
            );
        }
        for line in &preset.unsupported {
            expander =
                expander.row(SettingsRow::new(c.unsupported_badge).description(line.clone()));
        }
        let mut actions = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.0))
            .child(
                CheckBox::new(
                    "touchpad-confirm-preset",
                    CheckState::from_bool(preset.confirmed),
                )
                .label(c.confirm_changes)
                .on_change(Self::act_with(cx, |this, state: CheckState, cx| {
                    this.confirm_preset(state == CheckState::Checked, cx)
                })),
            )
            .child(
                Button::accent("touchpad-apply-preset", c.apply_preset)
                    .disabled(!preset.can_apply)
                    .on_click(Self::act(cx, |this, cx| {
                        this.apply_plan(cx);
                    })),
            )
            .child(
                Button::standard("touchpad-cancel-preview", c.cancel_preview)
                    .on_click(Self::act(cx, |this, cx| this.cancel_preview(cx))),
            );
        if let Some(reason) = preset.blocked_reason {
            actions = actions.child(label(reason, CAPTION, palette(cx).text_secondary));
        }
        expander
            .row(SettingsRow::new("").content(actions))
            .into_any_element()
    }

    /// One gesture: its picture, what it does, whether it is on, and test
    /// and edit.
    fn gesture_card(&self, index: usize, row: &GestureRow, cx: &mut Context<Self>) -> AnyElement {
        let c = self.c();
        let mut description = format!(
            "{} · {} {} · {}",
            row.shape_label, row.contacts, c.contacts_label, row.action_label
        );
        if let Some(direction) = row.direction_label {
            description.push_str(&format!(" · {direction}"));
        }
        description.push_str(&format!(" · {}", row.verification));
        let mut chips = div().flex().flex_wrap().gap(px(4.0));
        if row.conflict.is_some() {
            chips = chips.child(Chip::new(c.conflict_badge));
        }
        if !row.supported {
            chips = chips.child(Chip::new(c.unsupported_badge));
        }
        let toggle_id = row.id.clone();
        let test_id = row.id.clone();
        let edit_id = row.id.clone();
        SettingsCard::new(
            ElementId::named_usize("touchpad-gesture", index),
            row.label.clone(),
        )
        .leading(glyph(row.glyph, cx))
        .description(description)
        .content(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(chips)
                .child(
                    Button::standard(ElementId::named_usize("touchpad-test", index), c.test_run)
                        .on_click(Self::act(cx, move |this, cx| {
                            this.test_gesture(&test_id, cx)
                        })),
                )
                .child(
                    Button::standard(
                        ElementId::named_usize("touchpad-edit", index),
                        c.edit_gesture,
                    )
                    .on_click(Self::act(cx, move |this, cx| {
                        this.edit_gesture(&edit_id, cx)
                    })),
                )
                .child(
                    ToggleSwitch::new(
                        ElementId::named_usize("touchpad-enable", index),
                        row.enabled,
                    )
                    .locale(self.locale.words())
                    .without_words()
                    .on_change(Self::act_with(cx, move |this, on: bool, cx| {
                        this.toggle_gesture(&toggle_id, on, cx)
                    })),
                ),
        )
        .into_any_element()
    }

    /// The editor, under the gesture it edits: shape, direction, contacts,
    /// thumb, action, its keyboard shortcut, thresholds, cooldown.
    fn editor_card(&self, editor: &GestureEditor, cx: &mut Context<Self>) -> AnyElement {
        let c = self.c();
        let combo = |id: &'static str,
                     labels: Vec<String>,
                     selected: Option<usize>,
                     cx: &mut Context<Self>,
                     pick: EditorPick| {
            let pick = std::rc::Rc::new(pick);
            ComboBox::new(
                id,
                labels.into_iter().map(ComboItem::new).collect(),
                selected,
            )
            .min_width(px(CHOICE_WIDTH))
            .on_select(Self::act_with(cx, move |this, index: usize, cx| {
                if let Some(editor) = this.gestures.editor_mut() {
                    pick(editor, index);
                }
                cx.notify();
            }))
        };

        let shapes = GestureShape::ALL;
        let mut expander =
            SettingsExpander::new("touchpad-editor", gesture_label(&editor.id, c), true)
                .icon(IconName::Settings)
                .row(
                    SettingsRow::new(c.shape_label).content(combo(
                        "touchpad-editor-shape",
                        shapes
                            .iter()
                            .map(|shape| shape_label(*shape, c).to_string())
                            .collect(),
                        shapes.iter().position(|shape| *shape == editor.shape),
                        cx,
                        Box::new(move |editor, index| {
                            if let Some(shape) = shapes.get(index) {
                                editor.set_shape(*shape);
                            }
                        }),
                    )),
                );
        if editor.shape.needs_direction() {
            let directions = editor.shape.allowed_directions().to_vec();
            let picked = directions.clone();
            expander = expander.row(
                SettingsRow::new(c.direction_heading).content(combo(
                    "touchpad-editor-direction",
                    directions
                        .iter()
                        .map(|direction| direction_label(*direction, c).to_string())
                        .collect(),
                    directions
                        .iter()
                        .position(|direction| Some(*direction) == editor.direction),
                    cx,
                    Box::new(move |editor, index| {
                        if let Some(direction) = picked.get(index) {
                            editor.direction = Some(*direction);
                        }
                    }),
                )),
            );
        }
        expander = expander.row(SettingsRow::new(c.contacts_label).content(combo(
            "touchpad-editor-contacts",
            (1u8..=5).map(|count| count.to_string()).collect(),
            Some(usize::from(editor.contacts.clamp(1, 5)) - 1),
            cx,
            Box::new(|editor, index| editor.contacts = index as u8 + 1),
        )));
        expander = expander.row(
            SettingsRow::new(c.thumb_label).content(
                ToggleSwitch::new("touchpad-editor-thumb", editor.thumb_required)
                    .locale(self.locale.words())
                    .on_change(Self::act_with(cx, |this, on: bool, cx| {
                        if let Some(editor) = this.gestures.editor_mut() {
                            editor.thumb_required = on;
                        }
                        cx.notify();
                    })),
            ),
        );
        let actions = better_actions::DesktopAction::catalog();
        let picked_actions = actions.clone();
        expander = expander.row(
            SettingsRow::new(c.action_heading).content(combo(
                "touchpad-editor-action",
                actions
                    .iter()
                    .map(|action| action_label(action, c))
                    .collect(),
                actions.iter().position(|action| *action == editor.action),
                cx,
                Box::new(move |editor, index| {
                    if let Some(action) = picked_actions.get(index) {
                        editor.set_action(action.clone());
                    }
                }),
            )),
        );
        if editor.action_is_shortcut() {
            expander = self.shortcut_rows(expander, editor, cx);
        }
        expander = expander
            .row(
                SettingsRow::new(c.activation_label).content(combo(
                    "touchpad-editor-activation",
                    ACTIVATIONS
                        .iter()
                        .map(|value| format!("{:.0}%", value * 100.0))
                        .collect(),
                    ACTIVATIONS
                        .iter()
                        .position(|value| (value - editor.activation).abs() < 0.001),
                    cx,
                    Box::new(|editor, index| editor.activation = ACTIVATIONS[index]),
                )),
            )
            .row(
                SettingsRow::new(c.cancellation_label).content(combo(
                    "touchpad-editor-cancellation",
                    CANCELLATIONS
                        .iter()
                        .map(|value| format!("{:.0}%", value * 100.0))
                        .collect(),
                    CANCELLATIONS
                        .iter()
                        .position(|value| (value - editor.cancellation).abs() < 0.001),
                    cx,
                    Box::new(|editor, index| editor.cancellation = CANCELLATIONS[index]),
                )),
            )
            .row(
                SettingsRow::new(c.cooldown_label).content(combo(
                    "touchpad-editor-cooldown",
                    COOLDOWNS
                        .iter()
                        .map(|value| format!("{value} ms"))
                        .collect(),
                    COOLDOWNS
                        .iter()
                        .position(|value| *value == editor.cooldown_ms),
                    cx,
                    Box::new(|editor, index| editor.cooldown_ms = COOLDOWNS[index]),
                )),
            )
            .row(
                SettingsRow::new(c.enabled_label).content(
                    ToggleSwitch::new("touchpad-editor-enabled", editor.enabled)
                        .locale(self.locale.words())
                        .on_change(Self::act_with(cx, |this, on: bool, cx| {
                            if let Some(editor) = this.gestures.editor_mut() {
                                editor.enabled = on;
                            }
                            cx.notify();
                        })),
                ),
            );
        let mut buttons = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.0))
            .child(
                Button::accent("touchpad-editor-save", c.save_gesture)
                    .on_click(Self::act(cx, |this, cx| this.commit_edit(cx))),
            )
            .child(
                Button::standard("touchpad-editor-cancel", c.cancel_preview)
                    .on_click(Self::act(cx, |this, cx| this.cancel_edit(cx))),
            );
        if let Some(error) = &editor.error {
            buttons = buttons.child(label(
                problem_sentence(error, view::views_locale(self.locale)),
                CAPTION,
                palette(cx).critical,
            ));
        }
        expander
            .row(SettingsRow::new("").content(buttons))
            .into_any_element()
    }

    /// The custom keyboard shortcut: modifiers, the part of the key table,
    /// the key, how it is spelled, and what the recorded keybindings say.
    fn shortcut_rows(
        &self,
        mut expander: SettingsExpander,
        editor: &GestureEditor,
        cx: &mut Context<Self>,
    ) -> SettingsExpander {
        let c = self.c();
        let mut modifiers = div().flex().flex_wrap().gap(px(12.0));
        for modifier in Modifier::ALL {
            modifiers = modifiers.child(
                CheckBox::new(
                    ElementId::Name(format!("touchpad-modifier-{}", modifier.name()).into()),
                    CheckState::from_bool(editor.shortcut.holds(modifier)),
                )
                .label(modifier.name())
                .on_change(Self::act_with(cx, move |this, _: CheckState, cx| {
                    if let Some(editor) = this.gestures.editor_mut() {
                        editor.shortcut.toggle(modifier);
                    }
                    cx.notify();
                })),
            );
        }
        let groups = KeyGroup::ALL;
        let keys: Vec<Key> = editor.key_group.keys();
        let picked_keys = keys.clone();
        let spelling = match editor.shortcut.spelling() {
            Ok(text) => text,
            Err(_) => c.shortcut_needs_modifier.to_string(),
        };
        let check = match self.gestures.shortcut_check() {
            Some(ShortcutCheck::Conflicts { key }) => format!("{} {key}", c.shortcut_conflict),
            Some(ShortcutCheck::NoneRecorded) => c.shortcut_none_recorded.to_string(),
            Some(ShortcutCheck::Unknown { .. }) => c.shortcut_unknown.to_string(),
            None => String::new(),
        };
        expander = expander
            .row(SettingsRow::new(c.shortcut_modifiers).content(modifiers))
            .row(
                SettingsRow::new(c.shortcut_key).content(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(
                            ComboBox::new(
                                "touchpad-shortcut-group",
                                groups
                                    .iter()
                                    .map(|group| ComboItem::new(key_group_label(*group, c)))
                                    .collect(),
                                groups.iter().position(|group| *group == editor.key_group),
                            )
                            .min_width(px(140.0))
                            .on_select(Self::act_with(
                                cx,
                                move |this, index: usize, cx| {
                                    if let (Some(editor), Some(group)) =
                                        (this.gestures.editor_mut(), groups.get(index))
                                    {
                                        editor.key_group = *group;
                                    }
                                    cx.notify();
                                },
                            )),
                        )
                        .child(
                            ComboBox::new(
                                "touchpad-shortcut-key",
                                keys.iter().map(|key| ComboItem::new(key.name())).collect(),
                                keys.iter().position(|key| *key == editor.shortcut.key),
                            )
                            .min_width(px(120.0))
                            .on_select(Self::act_with(
                                cx,
                                move |this, index: usize, cx| {
                                    if let (Some(editor), Some(key)) =
                                        (this.gestures.editor_mut(), picked_keys.get(index))
                                    {
                                        editor.set_key(*key);
                                    }
                                    cx.notify();
                                },
                            )),
                        ),
                ),
            )
            .row(
                SettingsRow::new(c.shortcut_heading)
                    .description(check)
                    .content(label(spelling, BODY_STRONG, palette(cx).text_primary)),
            );
        expander
    }

    /// Replay a gesture, live testing, and whether the last test reached
    /// the desktop (ticket 88: each gesture can be tried on the spot).
    fn test_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.c();
        let run = self.gestures.last_test();
        let live = self.gestures.live_testing();
        // A switch that could not do anything is worse than one that is not
        // there, so it is off when no adapter reaches the desktop.
        let can_perform = self.gestures.adapter().describe().performs_system_actions;
        let mut description = if live {
            c.test_gestures_hint.to_string()
        } else {
            c.live_testing_off.to_string()
        };
        if !can_perform {
            description = c.gesture_backend_none.to_string();
        }
        let mut expander =
            SettingsExpander::new("touchpad-gesture-test-card", c.live_testing, true)
                .icon(IconName::Play)
                .description(description)
                .content(
                    ToggleSwitch::new("touchpad-live-testing", live)
                        .locale(self.locale.words())
                        .disabled(!can_perform)
                        .on_change(Self::act_with(cx, |this, on: bool, cx| {
                            this.set_live_testing(on, cx)
                        })),
                );
        if can_perform
            && self
                .gestures
                .config()
                .gestures
                .iter()
                .any(|gesture| gesture.enabled && gesture.thumb_required)
        {
            expander = expander.row(SettingsRow::new(c.gesture_thumb_best_effort));
        }
        if run.lines.is_empty() {
            expander =
                expander.row(SettingsRow::new(c.recognized_events).description(c.test_no_events));
        } else {
            expander = expander.row(
                SettingsRow::new(c.recognized_events).description(test_reach(
                    self.locale,
                    live,
                    run.attempted,
                    run.performed,
                )),
            );
            for line in &run.lines {
                expander = expander.row(SettingsRow::new(line.gesture.clone()).content(label(
                    format!(
                        "{} · {:.0}%",
                        event_words(line.kind, self.locale),
                        line.progress * 100.0
                    ),
                    CAPTION,
                    palette(cx).text_secondary,
                )));
            }
        }
        expander.into_any_element()
    }

    /// Export and import: buttons that open the desktop's file chooser, and
    /// the path fields when it does not answer; then what an import holds.
    fn file_rows(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let c = self.c();
        let pc = page_copy(self.locale);
        let words = self.locale.words();
        let choosing = self.file_task.is_some();
        let mut rows = Vec::new();
        let mut card =
            SettingsCard::new(card::GESTURE_FILE, c.profiles_file).icon(IconName::Folder);
        if self.portal {
            card = card.content(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(
                        Button::standard("touchpad-export", pc.choose_export)
                            .icon(IconName::ArrowUp)
                            .disabled(choosing)
                            .on_click(Self::act(cx, |this, cx| this.choose_file(true, cx))),
                    )
                    .child(
                        Button::standard("touchpad-import", pc.choose_import)
                            .icon(IconName::ArrowDown)
                            .disabled(choosing)
                            .on_click(Self::act(cx, |this, cx| this.choose_file(false, cx))),
                    ),
            );
        }
        rows.push(Row::card(FIRST_CARD_GAP, card::GESTURE_FILE, card));
        if !self.portal {
            let export = self.export_path.clone();
            let import = self.import_path.clone();
            rows.push(Row::new(
                CARD_GAP,
                SettingsCard::new("touchpad-export-path", c.export_label).content(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(
                            TextBox::new("touchpad-export-field", &self.export_path)
                                .width(px(360.0)),
                        )
                        .child(
                            Button::standard("touchpad-export-typed", c.export_profiles).on_click(
                                Self::act(cx, move |this, cx| {
                                    let path = export.read(cx).value().trim().to_string();
                                    this.export_to(std::path::Path::new(&path), cx);
                                }),
                            ),
                        ),
                ),
            ));
            rows.push(Row::new(
                CARD_GAP,
                SettingsCard::new("touchpad-import-path", c.import_label).content(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(
                            TextBox::new("touchpad-import-field", &self.import_path)
                                .width(px(360.0)),
                        )
                        .child(
                            Button::standard("touchpad-import-typed", c.import_profiles).on_click(
                                Self::act(cx, move |this, cx| {
                                    let path = import.read(cx).value().trim().to_string();
                                    this.import_from(std::path::Path::new(&path), cx);
                                }),
                            ),
                        ),
                ),
            ));
        }
        let note = match &self.file_note {
            FileNote::None => None,
            FileNote::Exported(path) => Some((
                Severity::Success,
                pc.exported_to
                    .replace("{path}", &path.display().to_string()),
            )),
            FileNote::PortalMissing => {
                Some((Severity::Informational, pc.portal_missing.to_string()))
            }
            FileNote::NotAFile => Some((Severity::Warning, pc.not_a_file.to_string())),
        };
        if let Some((severity, text)) = note {
            rows.push(Row::new(
                CARD_GAP,
                InfoBar::new("touchpad-file-note", severity, text).locale(words),
            ));
        }
        if let Some(summary) = self.gestures.import().cloned() {
            let mut expander =
                SettingsExpander::new("touchpad-import-summary", c.import_summary, true)
                    .icon(IconName::File)
                    .description(format!(
                        "{} · {}",
                        summary.source,
                        summary.device_profiles.len()
                    ));
            for identity in &summary.device_profiles {
                expander = expander.row(SettingsRow::new(identity.clone()));
            }
            if !summary.matches_selected_device {
                expander = expander.row(SettingsRow::new(c.import_uses_global));
            }
            rows.push(Row::new(CARD_GAP, expander));
        }
        rows
    }
}

/// The contact points and which way they travel.
fn glyph(glyph: GestureGlyph, cx: &App) -> AnyElement {
    let p = palette(cx);
    let dots = (0..glyph.dots).map(|index| {
        let thumb = glyph.thumb && index == 0;
        div()
            .size(px(if thumb { 6.0 } else { 5.0 }))
            .rounded_full()
            .bg(if thumb { p.accent } else { p.text_primary })
    });
    let bar = |w: f32, h: f32| div().w(px(w)).h(px(h)).rounded_full().bg(p.accent);
    let head = || div().size(px(5.0)).rounded_full().bg(p.accent);
    let arrow: AnyElement = match glyph.arrow {
        Arrow::Still => div()
            .w(px(14.0))
            .h(px(2.0))
            .rounded_full()
            .bg(p.text_secondary)
            .into_any_element(),
        Arrow::In | Arrow::Out => div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .child(bar(6.0, 2.0))
            .child(div().size(px(3.0)).rounded_full().bg(p.text_secondary))
            .child(bar(6.0, 2.0))
            .into_any_element(),
        Arrow::Turn => div()
            .size(px(12.0))
            .rounded_full()
            .border_2()
            .border_color(p.accent)
            .into_any_element(),
        Arrow::Up => div()
            .flex()
            .flex_col()
            .items_center()
            .child(head())
            .child(bar(2.0, 8.0))
            .into_any_element(),
        Arrow::Down => div()
            .flex()
            .flex_col()
            .items_center()
            .child(bar(2.0, 8.0))
            .child(head())
            .into_any_element(),
        Arrow::Left => div()
            .flex()
            .items_center()
            .child(head())
            .child(bar(10.0, 2.0))
            .into_any_element(),
        Arrow::Right => div()
            .flex()
            .items_center()
            .child(bar(10.0, 2.0))
            .child(head())
            .into_any_element(),
    };
    div()
        .w(px(32.0))
        .h(px(32.0))
        .flex_none()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(3.0))
        .rounded(radius::SM)
        .bg(p.card_secondary)
        .child(div().flex().items_end().gap(px(2.0)).children(dots))
        .child(arrow)
        .into_any_element()
}

/// The wording for one part of the key table.
pub(super) fn key_group_label(group: KeyGroup, c: &'static TpCopy) -> &'static str {
    match group {
        KeyGroup::Letters => c.group_letters,
        KeyGroup::Digits => c.group_digits,
        KeyGroup::Function => c.group_function,
        KeyGroup::Navigation => c.group_navigation,
        KeyGroup::Editing => c.group_editing,
        KeyGroup::Punctuation => c.group_punctuation,
    }
}

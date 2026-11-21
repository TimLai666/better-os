//! Draws Default apps with the Fluent kit: the page, a component filtered
//! from its details page, the review sub-page, and the results sub-page.
//!
//! These draw what `model` decided. No screen here asks the system anything,
//! and no control here changes anything: the command bar's two changes and
//! the overflow menu's two all open the review first, which is the whole
//! point of the feature.

use better_core::ComponentId;
use better_ui::fluent::breadcrumb::BreadcrumbBar;
use better_ui::fluent::button::{Button, DropDownButton};
use better_ui::fluent::command_bar::{Command, CommandBar};
use better_ui::fluent::info_bar::{ErrorBar, ErrorReport, InfoBar, Severity};
use better_ui::fluent::menu::MenuItem;
use better_ui::fluent::progress::{Progress, ProgressRing};
use better_ui::fluent::settings_card::{SettingsCard, SettingsExpander, SettingsRow};
use better_ui::fluent::toggle::{CheckBox, CheckState};
use better_ui::fluent::{BODY, CAPTION, TITLE, label, palette};
use better_ui::icon_set::Picture;
use better_ui::{StatusPill, StatusTone};
use defaults_core::{AggregateState, PlanKind, Selection};
use gpui::*;

use super::card;
use super::copy::page_copy;
use super::jobs::{component_name, now_seconds};
use super::model::{
    ComponentMenu, DefaultsRow, DefaultsSummary, MissingApplication, ResultTone, ReviewComponent,
    ReviewEntry, aggregate_label, changes_label, component_rows, integration_line,
    integration_state_label, kind_label, outcome_headline, outcome_tone, result_rows,
    session_effect_label, skip_reason_label, summary_line, visible_rows, warning_label,
};
use crate::app::ManagerApp;
use crate::i18n::{copy, shell};
use crate::model::Page;
use crate::render::{CARD_GAP, FIRST_CARD_GAP, GROUP_GAP, Row};

/// Lays out the controls in a card's header. The kit keeps a press on them
/// from also opening or closing the card (`settings_card::header_content`).
fn inside_header(element: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.0))
        .child(element)
}

fn element_id(prefix: &str, id: &ComponentId) -> ElementId {
    ElementId::Name(format!("{prefix}-{id}").into())
}

/// How a result or a run's headline is drawn. A setting that was left
/// alone reads as information, not as a success.
pub(crate) fn result_severity(tone: ResultTone) -> Severity {
    match tone {
        ResultTone::Success => Severity::Success,
        ResultTone::Pending | ResultTone::Neutral => Severity::Informational,
        ResultTone::Warning => Severity::Warning,
        ResultTone::Failure => Severity::Error,
    }
}

impl ManagerApp {
    /// How one component's overall state reads. The page shows its words as
    /// the expander's description; the tone is what a status takes.
    pub(crate) fn defaults_state_pill(
        locale: crate::i18n::Locale,
        aggregate: &AggregateState,
    ) -> StatusPill {
        let label = aggregate_label(locale, aggregate);
        let tone = match aggregate {
            AggregateState::Default => StatusTone::Success,
            AggregateState::NotDefault => StatusTone::Neutral,
            AggregateState::PartiallyDefault => StatusTone::Info,
            AggregateState::ChangedExternally => StatusTone::Warning,
            AggregateState::NeedsSignOut => StatusTone::Info,
            AggregateState::Conflict { .. } => StatusTone::Danger,
            AggregateState::Unavailable { .. } => StatusTone::Neutral,
            AggregateState::Unknown { .. } => StatusTone::Warning,
        };
        StatusPill::new(label, tone)
    }

    /// The rows every Default apps screen reads.
    fn defaults_rows(&self) -> Vec<DefaultsRow> {
        let Some(report) = self.defaults.report.as_ref() else {
            return Vec::new();
        };
        let names = self.defaults_names();
        let icons: std::collections::BTreeMap<ComponentId, better_core::ComponentIcon> = self
            .manager
            .manifests()
            .map(|manifest| (manifest.id.clone(), manifest.icon))
            .collect();
        component_rows(
            self.locale,
            report,
            &self.defaults.verified,
            &|component| component_name(&names, component),
            &|component| {
                icons
                    .get(component)
                    .copied()
                    .unwrap_or(better_core::ComponentIcon::Generic)
            },
        )
    }

    /// Whichever Default apps screen is open.
    pub(crate) fn default_apps_rows(&mut self, cx: &mut Context<Self>) -> Vec<Row> {
        match &self.page {
            Page::DefaultsReview => self.defaults_review_rows(cx),
            Page::DefaultsResults => self.defaults_results_rows(cx),
            Page::DefaultsComponent(component) => {
                let component = component.clone();
                self.defaults_main_rows(Some(&component), cx)
            }
            _ => self.defaults_main_rows(None, cx),
        }
    }

    /// `預設應用程式 › {crumb}`, as a sub-page has in place of its title.
    fn defaults_breadcrumb(
        &self,
        crumb: String,
        back: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Row {
        let s = shell(self.locale);
        Row::new(
            0.0,
            div().pb(px(4.0)).child(
                BreadcrumbBar::new(
                    "default-apps-breadcrumb",
                    vec![s.default_apps.into(), crumb.into()],
                )
                .style(TITLE)
                .on_click(Self::act_with(cx, move |this, index: usize, _, cx| {
                    if index == 0 {
                        back(this, cx);
                    }
                })),
            ),
        )
    }

    fn defaults_description(&self, text: &'static str, cx: &App) -> Row {
        Row::new(
            0.0,
            label(text, BODY, palette(cx).text_secondary).pb(px(12.0)),
        )
    }

    /// The states every screen of the page shares: a read that failed, and
    /// a job that has run long enough for a ring.
    fn defaults_state_rows(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let c = copy(self.locale);
        let mut rows = Vec::new();
        if self.defaults.failed {
            rows.push(Row::new(
                0.0,
                div().pb(px(12.0)).child(
                    ErrorBar::new(
                        "default-apps-failed",
                        ErrorReport::new(c.defaults_read_failed, String::new()),
                    )
                    .locale(self.locale.words()),
                ),
            ));
        }
        if self.defaults.shows_ring() {
            rows.push(Row::new(
                0.0,
                div()
                    .pb(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        ProgressRing::new("default-apps-reading", Progress::Indeterminate)
                            .size(px(32.0)),
                    )
                    .child(label(
                        c.defaults_working,
                        CAPTION,
                        palette(cx).text_secondary,
                    )),
            ));
        }
        rows
    }

    /// The page, or one component of it when a details page linked here.
    fn defaults_main_rows(&self, only: Option<&ComponentId>, cx: &mut Context<Self>) -> Vec<Row> {
        let locale = self.locale;
        let c = copy(locale);
        let w = page_copy(locale);
        let s = shell(locale);
        let p = palette(cx);
        let busy = self.defaults.busy;
        let rows = visible_rows(self.defaults_rows(), only);
        let mut out = Vec::new();

        match only {
            Some(component) => {
                let name = component_name(&self.defaults_names(), component);
                out.push(self.defaults_breadcrumb(
                    name,
                    |this, cx| this.navigate(Page::Defaults, cx),
                    cx,
                ));
                out.push(self.defaults_description(w.description, cx));
            }
            None => {
                out.push(Row::new(
                    0.0,
                    label(s.default_apps, TITLE, p.text_primary).pb(px(4.0)),
                ));
                out.push(self.defaults_description(w.description, cx));
            }
        }
        out.extend(self.defaults_state_rows(cx));

        if only.is_none()
            && let Some(report) = self.defaults.report.as_ref()
        {
            out.push(Row::card(
                0.0,
                card::SUMMARY,
                label(
                    summary_line(locale, &DefaultsSummary::of(report)),
                    BODY,
                    p.text_secondary,
                )
                .pb(px(8.0)),
            ));
        }

        let selection = match only {
            Some(component) => Selection::one(component.clone()),
            None => Selection::All,
        };
        let none = rows.is_empty() || busy;
        let (apply, restore, verify) = (selection.clone(), selection.clone(), selection);
        out.push(Row::card(
            0.0,
            card::COMMANDS,
            CommandBar::new("default-apps-commands")
                .locale(locale.words())
                .command(
                    Command::new(c.use_better_defaults, Picture::SetDefault)
                        .disabled(none)
                        .on_click(Self::act(cx, move |this, _, cx| {
                            this.review_defaults(PlanKind::Apply, apply.clone(), cx)
                        })),
                )
                .command(
                    Command::new(c.restore_previous_defaults, Picture::Restore)
                        .disabled(none)
                        .on_click(Self::act(cx, move |this, _, cx| {
                            this.review_defaults(PlanKind::Restore, restore.clone(), cx)
                        })),
                )
                .command(
                    Command::new(c.verify_again, Picture::Refresh)
                        .disabled(none)
                        .on_click(Self::act(cx, move |this, _, cx| {
                            this.verify_defaults(verify.clone(), cx)
                        })),
                ),
        ));

        // Nothing declares a default: one sentence, and the way to the
        // components that could.
        if rows.is_empty() && !busy && !self.defaults.failed && self.defaults.report.is_some() {
            out.push(Row::new(
                FIRST_CARD_GAP,
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .child(label(
                        format!("{} {}", c.defaults_empty_title, c.defaults_empty_detail),
                        BODY,
                        p.text_secondary,
                    ))
                    .child(
                        div().flex().child(
                            Button::standard("default-apps-empty", s.apps)
                                .on_click(Self::act(cx, |this, _, cx| {
                                    this.navigate(Page::Components, cx)
                                })),
                        ),
                    ),
            ));
            return out;
        }

        for (index, row) in rows.iter().enumerate() {
            let expander = self.defaults_expander(row, only.is_some(), cx);
            let gap = if index == 0 { FIRST_CARD_GAP } else { CARD_GAP };
            out.push(if index == 0 {
                Row::card(gap, card::COMPONENTS, expander)
            } else {
                Row::new(gap, expander)
            });
        }
        out
    }

    /// One component: its state, a row per integration, and its overflow
    /// menu of Make default, Restore, and Check again.
    fn defaults_expander(
        &self,
        row: &DefaultsRow,
        filtered: bool,
        cx: &mut Context<Self>,
    ) -> SettingsExpander {
        let locale = self.locale;
        let c = copy(locale);
        let w = page_copy(locale);
        let p = palette(cx);
        let busy = self.defaults.busy;
        let menu = ComponentMenu::of(row);
        let id = row.component.clone();
        let (apply, restore, verify, toggle) = (id.clone(), id.clone(), id.clone(), id.clone());
        // A component that already is the default gets no Make default, and
        // one with nothing saved no Restore: an entry that could never do
        // anything is not drawn, not drawn disabled.
        let mut entries = Vec::new();
        if menu.make_default {
            entries.push(
                MenuItem::new(c.make_default)
                    .on_select(Self::act(cx, move |this, _, cx| {
                        this.review_defaults(PlanKind::Apply, Selection::one(apply.clone()), cx)
                    }))
                    .entry(),
            );
        }
        if menu.restore {
            entries.push(
                MenuItem::new(w.restore)
                    .on_select(Self::act(cx, move |this, _, cx| {
                        this.review_defaults(PlanKind::Restore, Selection::one(restore.clone()), cx)
                    }))
                    .entry(),
            );
        }
        entries.push(
            MenuItem::new(c.verify_again)
                .on_select(Self::act(cx, move |this, _, cx| {
                    this.verify_defaults(Selection::one(verify.clone()), cx)
                }))
                .entry(),
        );
        let open = filtered || self.defaults.open.contains(&row.component);
        let mut expander = SettingsExpander::new(
            ElementId::Name(row.element_id("default-apps-component").into()),
            row.name.clone(),
            open,
        )
        .icon(self.component_icon(row.icon))
        .description(Self::defaults_state_pill(locale, &row.aggregate).label)
        .content(inside_header(
            DropDownButton::more(
                ElementId::Name(row.element_id("default-apps-more").into()),
                w.more_actions.replace("{component}", &row.name),
            )
            .entries(entries)
            .disabled(busy)
            .align_right(),
        ))
        .on_toggle(Self::act_with(cx, move |this, open: bool, _, cx| {
            this.set_defaults_open(&toggle, open, cx)
        }));
        let now = now_seconds();
        for integration in &row.integrations {
            expander = expander.row(
                SettingsRow::new(kind_label(locale, integration.kind))
                    .description(integration_line(locale, integration, now))
                    .content(label(
                        integration_state_label(locale, &integration.state),
                        BODY,
                        p.text_secondary,
                    )),
            );
        }
        expander
    }

    // ---- Review ------------------------------------------------------------

    fn defaults_review_rows(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let locale = self.locale;
        let c = copy(locale);
        let w = page_copy(locale);
        let p = palette(cx);
        let Some(review) = self.defaults.review.as_ref() else {
            // A review that is not there any more (its plan was being made
            // again, or the page was restored after an update).
            let mut rows = vec![self.defaults_breadcrumb(
                c.defaults_review_title.to_string(),
                |this, cx| this.cancel_defaults_review(cx),
                cx,
            )];
            rows.extend(self.defaults_state_rows(cx));
            if !self.defaults.busy {
                rows.push(Row::new(
                    0.0,
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .child(label(c.nothing_to_change, BODY, p.text_secondary))
                        .child(div().flex().child(
                            Button::standard("default-apps-review-back", c.back).on_click(
                                Self::act(cx, |this, _, cx| this.cancel_defaults_review(cx)),
                            ),
                        )),
                ));
            }
            return rows;
        };
        let restoring = review.kind() == PlanKind::Restore;
        let summary = review.summary();
        let elevation = review.elevation();
        let approval = review.approve().is_some();

        let mut rows = vec![
            self.defaults_breadcrumb(
                if restoring {
                    c.restore_review_title
                } else {
                    c.defaults_review_title
                }
                .to_string(),
                |this, cx| this.cancel_defaults_review(cx),
                cx,
            ),
            self.defaults_description(
                if restoring {
                    c.restore_review_subtitle
                } else {
                    c.defaults_review_subtitle
                },
                cx,
            ),
        ];
        rows.extend(self.defaults_state_rows(cx));

        for (index, component) in review.components().iter().enumerate() {
            rows.extend(self.defaults_review_component(
                component,
                restoring,
                if index == 0 { 0.0 } else { GROUP_GAP },
                cx,
            ));
        }

        // The bottom summary.
        let count = |value: usize| label(value.to_string(), BODY, p.text_secondary);
        let mut lines = SettingsExpander::new(
            "default-apps-review-summary",
            w.review_summary,
            !self.defaults.summary_closed,
        )
        .icon(Picture::About)
        .description(if elevation.requested {
            c.skip_requires_administrator
        } else {
            c.no_elevated_access
        })
        .on_toggle(Self::act_with(cx, |this, open: bool, _, cx| {
            this.defaults.summary_closed = !open;
            cx.notify();
        }))
        .row(SettingsRow::new(c.components_selected).content(count(summary.components_selected)))
        .row(SettingsRow::new(c.settings_affected).content(count(summary.settings_affected)))
        .row(SettingsRow::new(c.state_needs_sign_out).content(count(summary.needs_sign_out)))
        .row(SettingsRow::new(c.effect_restart).content(count(summary.needs_restart)))
        .row(SettingsRow::new(c.snapshot_label).content(label(
            if summary.will_capture > 0 {
                c.snapshot_will_capture
            } else {
                c.snapshot_nothing_to_capture
            },
            BODY,
            p.text_secondary,
        )))
        .row(SettingsRow::new(c.manual_action_required).content(count(summary.manual_actions)))
        .row(
            SettingsRow::new(c.awaiting_confirmation).content(count(summary.awaiting_confirmation)),
        );
        if summary.damaged_snapshots > 0 {
            lines = lines.row(
                SettingsRow::new(c.damaged_snapshots).content(count(summary.damaged_snapshots)),
            );
        }
        if elevation.excluded_needing_administrator > 0 {
            lines = lines.row(
                SettingsRow::new(c.elevated_excluded)
                    .content(count(elevation.excluded_needing_administrator)),
            );
        }
        rows.push(Row::new(GROUP_GAP, lines));

        let mut footer = div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_end()
            .gap(px(8.0));
        if !approval {
            footer = footer.child(label(c.nothing_to_change, CAPTION, p.text_secondary));
        }
        footer = footer
            .child(
                Button::standard("default-apps-review-cancel", c.cancel)
                    .on_click(Self::act(cx, |this, _, cx| this.cancel_defaults_review(cx))),
            )
            .child(
                Button::accent(
                    "default-apps-review-apply",
                    if restoring {
                        c.restore_selected_defaults
                    } else {
                        c.apply_selected_defaults
                    },
                )
                .disabled(!approval || self.defaults.busy)
                .on_click(Self::act(cx, |this, _, cx| this.apply_defaults_review(cx))),
            );
        rows.push(Row::new(GROUP_GAP, footer));
        rows
    }

    /// One component on the review: its check box, then each of its entries
    /// indented under it.
    fn defaults_review_component(
        &self,
        component: &ReviewComponent,
        restoring: bool,
        gap: f32,
        cx: &mut Context<Self>,
    ) -> Vec<Row> {
        let locale = self.locale;
        let id = component.component.clone();
        let mut rows = vec![Row::new(
            gap,
            SettingsCard::new(
                element_id("default-apps-review", &component.component),
                component.name.clone(),
            )
            .leading(
                CheckBox::new(
                    element_id("default-apps-select", &component.component),
                    CheckState::from_bool(component.selected),
                )
                .disabled(self.defaults.busy)
                .on_change(Self::act_with(cx, move |this, _: CheckState, _, cx| {
                    this.toggle_defaults_component(&id, cx)
                })),
            )
            .description(changes_label(locale, component.changes)),
        )];
        for entry in &component.entries {
            rows.extend(self.defaults_review_entry(entry, restoring, cx));
        }
        rows
    }

    fn defaults_review_entry(
        &self,
        entry: &ReviewEntry,
        restoring: bool,
        cx: &mut Context<Self>,
    ) -> Vec<Row> {
        let locale = self.locale;
        let c = copy(locale);
        let p = palette(cx);
        let indent = |element: AnyElement| div().pl(px(40.0)).child(element);
        let confirmed =
            entry.confirmed
                || self.defaults.review.as_ref().is_some_and(|review| {
                    review.is_confirmed(&entry.component, &entry.integration)
                });

        let mut description = vec![
            format!(
                "{}{}{}",
                c.current_value, c.label_separator, entry.current_owner
            ),
            format!("{}{}{}", c.new_value, c.label_separator, entry.new_owner),
            session_effect_label(locale, entry.session_effect).to_string(),
            if entry.restorable {
                c.can_be_restored
            } else {
                c.cannot_be_restored
            }
            .to_string(),
        ];
        if let Some(reason) = entry.skipped.as_ref() {
            description.push(format!(
                "{}{}{}",
                c.nothing_to_change,
                c.label_separator,
                skip_reason_label(locale, reason)
            ));
        }
        let mut card = SettingsCard::new(
            ElementId::Name(entry.element_id("default-apps-entry").into()),
            kind_label(locale, entry.kind),
        )
        .description(description.join(" · "));
        if entry.requires_confirmation {
            let (component, integration) = (entry.component.clone(), entry.integration.clone());
            card = card.content(
                CheckBox::new(
                    ElementId::Name(entry.element_id("default-apps-confirm").into()),
                    CheckState::from_bool(confirmed),
                )
                .label(c.confirm_overwrite)
                .disabled(self.defaults.busy)
                .on_change(Self::act_with(
                    cx,
                    move |this, _: CheckState, _, cx| {
                        this.toggle_defaults_confirmation(&component, &integration, cx)
                    },
                )),
            );
        } else if restoring {
            card = card.content(label(
                entry.restore_class.label(locale),
                BODY,
                p.text_secondary,
            ));
        }
        let mut rows = vec![Row::new(CARD_GAP, indent(card.into_any_element()))];
        for warning in &entry.warnings {
            rows.push(Row::new(
                CARD_GAP,
                indent(
                    InfoBar::new(
                        ElementId::Name(
                            format!("{}-{warning:?}", entry.element_id("default-apps-warning"))
                                .into(),
                        ),
                        Severity::Warning,
                        warning_label(locale, warning),
                    )
                    .locale(locale.words())
                    .into_any_element(),
                ),
            ));
        }
        for (index, missing) in entry.missing.iter().enumerate() {
            rows.push(Row::new(
                CARD_GAP,
                indent(self.defaults_missing_application(entry, index, missing, cx)),
            ));
        }
        rows
    }

    /// One application a restore cannot put back: which one, a button for
    /// each installed application that opens the type, and one that leaves
    /// it without a default. The chosen one is the accent button, and
    /// pressing it again takes the choice back.
    fn defaults_missing_application(
        &self,
        entry: &ReviewEntry,
        index: usize,
        missing: &MissingApplication,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let busy = self.defaults.busy;
        let mut buttons = div().flex().flex_wrap().gap(px(8.0));
        for (position, option) in missing.options.iter().enumerate() {
            let component = entry.component.clone();
            let integration = entry.integration.clone();
            let key = missing.key.clone();
            let choice = option.choice.clone();
            let id = ElementId::Name(
                format!(
                    "{}-{index}-{position}",
                    entry.element_id("default-apps-restore-instead")
                )
                .into(),
            );
            let button = if option.chosen {
                Button::accent(id, option.label.clone())
            } else {
                Button::standard(id, option.label.clone())
            };
            buttons = buttons.child(button.disabled(busy).on_click(Self::act(
                cx,
                move |this, _, cx| {
                    this.choose_defaults_restore(
                        &component,
                        &integration,
                        key.as_deref(),
                        choice.clone(),
                        cx,
                    )
                },
            )));
        }
        // The last option is always clearing, so fewer than two means no
        // installed application opens the type.
        let no_alternative = missing.options.len() < 2;
        SettingsCard::new(
            ElementId::Name(format!("{}-{index}", entry.element_id("default-apps-missing")).into()),
            missing.headline.clone(),
        )
        .status(Severity::Warning)
        .description(if no_alternative {
            format!("{} · {}", c.no_alternative_application, c.restore_instead)
        } else {
            c.restore_instead.to_string()
        })
        .content(buttons)
        .into_any_element()
    }

    // ---- Results -----------------------------------------------------------

    fn defaults_results_rows(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let locale = self.locale;
        let c = copy(locale);
        let p = palette(cx);
        let mut rows = vec![
            self.defaults_breadcrumb(
                c.defaults_results_title.to_string(),
                |this, cx| {
                    let origin = this.defaults_origin();
                    this.navigate(origin, cx)
                },
                cx,
            ),
            self.defaults_description(c.defaults_results_subtitle, cx),
        ];
        let Some(outcome) = self.defaults.outcome.as_ref() else {
            rows.extend(self.defaults_state_rows(cx));
            return rows;
        };
        let names = self.defaults_names();
        let results = result_rows(locale, outcome, &|component| {
            component_name(&names, component)
        });
        rows.push(Row::new(
            0.0,
            div().pb(px(12.0)).child(
                InfoBar::new(
                    "default-apps-outcome",
                    result_severity(outcome_tone(outcome)),
                    outcome_headline(locale, outcome),
                )
                .locale(locale.words()),
            ),
        ));
        // Each result is one setting: named by its kind, under its
        // component, as the review named it.
        let kinds: std::collections::BTreeMap<_, _> = self
            .manager
            .manifests()
            .flat_map(|manifest| {
                manifest
                    .default_integrations
                    .iter()
                    .map(|integration| {
                        (
                            (manifest.id.clone(), integration.id.clone()),
                            integration.kind,
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        for (index, result) in results.iter().enumerate() {
            let header = kinds
                .get(&(result.component.clone(), result.integration.clone()))
                .map(|kind| kind_label(locale, *kind).to_string())
                .unwrap_or_else(|| result.integration.to_string());
            let detail = result.detail.clone().unwrap_or_else(|| c.none.to_string());
            let card = SettingsCard::new(
                ElementId::Name(
                    format!(
                        "default-apps-result-{}-{}",
                        result.component, result.integration
                    )
                    .into(),
                ),
                header,
            )
            .status(result_severity(result.tone))
            .description(format!("{} · {detail}", result.name))
            .content(label(result.label, BODY, p.text_secondary));
            rows.push(Row::new(if index == 0 { 0.0 } else { CARD_GAP }, card));
        }
        rows.push(Row::new(
            GROUP_GAP,
            div().flex().justify_end().child(
                Button::accent("default-apps-results-back", c.back).on_click(Self::act(
                    cx,
                    |this, _, cx| {
                        let origin = this.defaults_origin();
                        this.navigate(origin, cx)
                    },
                )),
            ),
        ));
        rows
    }
}

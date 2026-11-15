//! Draws Apps (spec section 7): the component list, a component's page,
//! Updates, Health, Activity, and the four confirmation dialogs, all on
//! `better_ui::fluent` controls. What each row says and offers is decided
//! in `apps`, with no GPUI; this file only draws it.

use better_core::ComponentId;
use better_ui::fluent::breadcrumb::BreadcrumbBar;
use better_ui::fluent::button::Button;
use better_ui::fluent::dialog::{ContentDialog, DefaultButton, DialogButtons, DialogResult};
use better_ui::fluent::hyperlink::HyperlinkButton;
use better_ui::fluent::info_bar::{ErrorBar, ErrorReport, InfoBar, Severity};
use better_ui::fluent::list::ListRow;
use better_ui::fluent::progress::{Progress, ProgressBar, ProgressRing};
use better_ui::fluent::settings_card::{SettingsCard, SettingsExpander, SettingsRow};
use better_ui::fluent::tabs::{SelectorBar, SelectorItem};
use better_ui::fluent::text_box::TextBox;
use better_ui::fluent::toggle::{CheckBox, CheckState};
use better_ui::fluent::{BODY, CAPTION, TITLE, label, palette};
use gpui::*;
use gpui_component::IconName;
use manager_core::catalog::now_unix_seconds;
use manager_core::{
    ComponentStatus, DesiredOperation, DoctorCheckKind, DoctorCheckStatus, RecoveryStatus,
    is_self_component,
};

use crate::app::ManagerApp;
use crate::apps::{self, Dialog, Trailing};
use crate::i18n::{copy, shell};
use crate::model::{
    ActivityFilter, CatalogLine, ComponentInfo, DetailTab, Page, UpdateCheck,
    package_system_notice, repair_result_line, unorderable_detail,
};
use crate::pages;
use crate::render::{CARD_GAP, FIRST_CARD_GAP, GROUP_GAP, Row};

/// Keeps a press on a control inside a clickable card from also opening
/// the card's page.
fn inside_card(element: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(element)
}

fn element_id(prefix: &str, id: &ComponentId) -> ElementId {
    ElementId::Name(format!("{prefix}-{id}").into())
}

impl ManagerApp {
    /// The rows of whichever Apps page is open.
    pub(crate) fn apps_rows(&self, cx: &mut Context<Self>) -> Vec<Row> {
        match &self.page {
            Page::ComponentDetail(id) => self.component_page_rows(&id.clone(), cx),
            Page::Updates => self.updates_rows(cx),
            Page::Health => self.health_rows(cx),
            Page::Activity => self.activity_rows(cx),
            _ => self.apps_root_rows(cx),
        }
    }

    /// `應用程式 › {page}` in place of the title, as a sub-page has.
    fn breadcrumb_row(&self, crumb: String, cx: &mut Context<Self>) -> Row {
        let s = shell(self.locale);
        Row::new(
            0.0,
            div().pb(px(12.0)).child(
                BreadcrumbBar::new("apps-breadcrumb", vec![s.apps.into(), crumb.into()])
                    .style(TITLE)
                    .on_click(Self::act_with(cx, |this, index: usize, _, cx| {
                        if index == 0 {
                            this.navigate(Page::Components, cx);
                        }
                    })),
            ),
        )
    }

    /// What every Apps page says first: a change that could not be planned,
    /// what happened to a removed component's data, and the running change.
    fn apps_notices(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let c = copy(self.locale);
        let a = apps::copy(self.locale);
        let mut rows = Vec::new();
        if self.planning_error == Some(crate::app::AppError::Planning) {
            rows.push(Row::new(
                0.0,
                div().pb(px(12.0)).child(
                    ErrorBar::new(
                        "planning-error",
                        ErrorReport::new(c.planning_failed_detail, String::new()),
                    )
                    .locale(self.locale.words())
                    .on_close(Self::act(cx, |this, _, cx| {
                        this.planning_error = None;
                        cx.notify();
                    })),
                ),
            ));
        }
        if let Some((id, report)) = &self.data_removal {
            let name = self.plan_component_name(id);
            let bar = if report.succeeded() {
                InfoBar::new(
                    "data-removal",
                    Severity::Success,
                    a.data_trashed.replace("{component}", &name),
                )
            } else {
                InfoBar::new(
                    "data-removal",
                    Severity::Warning,
                    a.data_failed.replace("{component}", &name),
                )
                .message(
                    report
                        .failed
                        .iter()
                        .map(|(path, reason)| format!("{} ({reason})", path.display()))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            };
            rows.push(Row::new(
                0.0,
                div()
                    .pb(px(12.0))
                    .child(bar.locale(self.locale.words()).on_close(Self::act(
                        cx,
                        |this, _, cx| {
                            this.data_removal = None;
                            cx.notify();
                        },
                    ))),
            ));
        }
        if let Some(row) = self.running_card(cx) {
            rows.push(row);
        }
        rows
    }

    /// The change that is running: what it does, to how many, its stage, and
    /// Cancel while cancelling can still put the machine back.
    fn running_card(&self, cx: &mut Context<Self>) -> Option<Row> {
        let run = self.run_facts()?;
        let a = apps::copy(self.locale);
        let mut description = self.stage_label(run.stage).to_string();
        if !self.is_demo() && run.stage == manager_core::OperationStage::Downloading {
            description = format!("{description} · {}", a.run_password);
        }
        let mut actions = div().flex().items_center().gap(px(8.0));
        let interrupted = !self.is_demo() && self.running.is_none();
        if interrupted {
            actions = actions.child(
                Button::standard("run-continue", copy(self.locale).continue_label)
                    .on_click(Self::act(cx, |this, _, cx| this.resume_transaction(cx))),
            );
        }
        if self.can_cancel_now() {
            let cancelling = self
                .cancel
                .as_ref()
                .is_some_and(|token| token.is_cancelled());
            actions = actions.child(
                Button::standard(
                    "run-cancel",
                    if cancelling {
                        a.run_cancelling
                    } else {
                        a.run_cancel
                    },
                )
                .disabled(cancelling)
                .on_click(Self::act(cx, |this, _, cx| this.cancel_install(cx))),
            );
        }
        Some(Row::new(
            0.0,
            div().pb(px(12.0)).child(
                SettingsCard::new("running-change", apps::run_title(self.locale, run.steps))
                    .leading(
                        ProgressRing::new("running-ring", Progress::Indeterminate).size(px(20.0)),
                    )
                    .description(description)
                    .content(actions),
            ),
        ))
    }

    /// What a failed row says: the stage's words for what went wrong, or
    /// that a health check did not pass.
    fn failure_sentence(&self, component: &ComponentInfo) -> String {
        match &component.failure {
            Some(failure) => {
                let (key, detail) = failure.evidence_parts();
                let label = self.evidence_label(Some(key), detail);
                if label == copy(self.locale).none {
                    copy(self.locale).last_install_failed.to_string()
                } else {
                    label.to_string()
                }
            }
            None => apps::copy(self.locale).failed_health.to_string(),
        }
    }

    /// The trailing edge of a component's row or header card.
    fn trailing_element(
        &self,
        component: &ComponentInfo,
        trailing: &Trailing,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        let a = apps::copy(self.locale);
        let busy = self.transaction_running();
        let id = component.core_id.clone();
        match trailing {
            Trailing::Progress(progress) => div()
                .w(px(220.0))
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(label(
                    progress.caption(self.locale),
                    CAPTION,
                    p.text_secondary,
                ))
                .child(ProgressBar::new(
                    element_id("row-progress", &id),
                    match progress.fraction() {
                        Some(fraction) => Progress::Determinate(fraction),
                        None => Progress::Indeterminate,
                    },
                ))
                .into_any_element(),
            Trailing::Failed { retry, restore } => {
                let retry = *retry;
                let retry_id = id.clone();
                let mut buttons = div().flex().items_center().gap(px(8.0)).child(
                    Button::standard(element_id("row-retry", &id), a.action_retry)
                        .disabled(busy)
                        .on_click(Self::act(cx, move |this, _, cx| {
                            this.request_operation(&retry_id, retry, cx)
                        })),
                );
                if *restore {
                    let restore_id = id.clone();
                    buttons = buttons.child(
                        Button::standard(element_id("row-restore", &id), a.action_restore)
                            .disabled(busy)
                            .on_click(Self::act(cx, move |this, _, cx| {
                                this.request_operation(&restore_id, DesiredOperation::Restore, cx)
                            })),
                    );
                }
                inside_card(buttons).into_any_element()
            }
            Trailing::Result(line) => div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_color(p.success)
                .child(gpui_component::Icon::new(IconName::CircleCheck).size(px(16.0)))
                .child(label(line.clone(), BODY, p.text_secondary))
                .into_any_element(),
            Trailing::Action(operation) => {
                let operation = *operation;
                inside_card(
                    Button::standard(
                        element_id("row-action", &id),
                        apps::action_label(self.locale, operation),
                    )
                    .disabled(busy)
                    .on_click(Self::act(cx, move |this, _, cx| {
                        this.request_operation(&id, operation, cx)
                    })),
                )
                .into_any_element()
            }
            Trailing::View => inside_card(
                Button::standard(element_id("row-view", &id), a.action_view)
                    .on_click(Self::act(cx, move |this, _, cx| {
                        this.open_component(&id, cx)
                    })),
            )
            .into_any_element(),
            Trailing::State(word) => label(word.clone(), BODY, p.text_secondary).into_any_element(),
        }
    }

    /// One component as a settings card with a chevron to its page.
    fn component_card(&self, component: &ComponentInfo, cx: &mut Context<Self>) -> SettingsCard {
        let trailing = self.row_trailing(component);
        let description = match &trailing {
            Trailing::Failed { .. } => self.failure_sentence(component),
            _ => self.declared_or_not(&component.summary),
        };
        let id = component.core_id.clone();
        let mut card = SettingsCard::new(element_id("component", &id), component.name.clone())
            .description(description)
            .content(self.trailing_element(component, &trailing, cx))
            .on_click(Self::act(cx, move |this, _, cx| {
                this.open_component(&id, cx)
            }));
        card = if matches!(trailing, Trailing::Failed { .. }) {
            card.status(Severity::Error)
        } else {
            card.icon(self.component_glyph(component))
        };
        card
    }

    /// Apps: the three sub-page cards, the filter, the search, the catalog
    /// line, and the components.
    fn apps_root_rows(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let s = shell(self.locale);
        let a = apps::copy(self.locale);
        let p = palette(cx);
        let mut rows = vec![self.title_row(s.apps, cx)];
        rows.extend(self.apps_notices(cx));

        let links = pages::apps_links(self.locale, self.update_plan_count());
        for (index, link) in links.into_iter().enumerate() {
            let page = link.page.clone();
            rows.push(Row::card(
                if index == 0 { 0.0 } else { CARD_GAP },
                link.card,
                SettingsCard::new(link.card, link.header)
                    .icon(match link.page {
                        Page::Updates => IconName::ArrowDown,
                        Page::Health => IconName::CircleCheck,
                        _ => IconName::Inspector,
                    })
                    .description(link.description)
                    .on_click(Self::act(cx, move |this, _, cx| {
                        this.navigate(page.clone(), cx)
                    })),
            ));
        }

        rows.push(Row::card(
            GROUP_GAP,
            pages::apps_card::COMPONENTS,
            label(
                a.group_components,
                better_ui::fluent::BODY_STRONG,
                p.text_primary,
            ),
        ));
        let filter = self.state.settings.component_filter;
        let selected = apps::FILTERS.iter().position(|value| *value == filter);
        let items = apps::FILTERS
            .iter()
            .map(|filter| SelectorItem::new(apps::filter_label(self.locale, *filter)))
            .collect();
        rows.push(Row::new(
            FIRST_CARD_GAP,
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(
                    SelectorBar::new("component-filter", items, selected).on_select(
                        Self::act_with(cx, |this, index: usize, _, cx| {
                            if let Some(filter) = apps::FILTERS.get(index) {
                                this.set_component_filter(*filter, cx);
                            }
                        }),
                    ),
                )
                .child(TextBox::search("component-search", &self.search).width(px(260.0))),
        ));

        // {age} · [Refresh list], and the list's warning under it.
        let line = CatalogLine::present(self.locale, &self.catalog_status, now_unix_seconds());
        let mut catalog = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(4.0))
            .child(label(line.age.clone(), CAPTION, p.text_secondary))
            .child(label("·", CAPTION, p.text_secondary));
        catalog = if self.catalog_refreshing {
            catalog.child(label(a.refreshing, CAPTION, p.text_secondary))
        } else {
            catalog.child(
                HyperlinkButton::new("refresh-list", a.refresh_list)
                    .on_click(Self::act(cx, |this, _, cx| this.refresh_catalog(cx))),
            )
        };
        rows.push(Row::card(CARD_GAP, pages::apps_card::CATALOG, catalog));
        if let Some(warning) = line.warning {
            rows.push(Row::new(0.0, label(warning, CAPTION, p.caution)));
        }
        if let Some(rejected) = line.rejected_line(self.locale) {
            rows.push(Row::new(0.0, label(rejected, CAPTION, p.text_secondary)));
        }

        let visible: Vec<ComponentInfo> = self
            .components()
            .into_iter()
            .filter(|component| apps::matches_filter(component, filter))
            .filter(|component| apps::matches_search(component, &self.search_query))
            .collect();
        if visible.is_empty() {
            let query = self.search_query.trim();
            let (sentence, button) = if query.is_empty() {
                (a.empty_view.to_string(), a.show_all)
            } else {
                (a.no_match.replace("{query}", query), a.clear_search)
            };
            rows.push(Row::new(
                FIRST_CARD_GAP,
                div()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(px(8.0))
                    .child(label(sentence, BODY, p.text_secondary))
                    .child(
                        Button::standard("components-empty", button).on_click(Self::act(
                            cx,
                            |this, window, cx| {
                                this.clear_search(window, cx);
                                this.set_component_filter(
                                    manager_core::ComponentFilterPreference::All,
                                    cx,
                                );
                            },
                        )),
                    ),
            ));
        }
        for (index, component) in visible.iter().enumerate() {
            rows.push(Row::new(
                if index == 0 { FIRST_CARD_GAP } else { CARD_GAP },
                self.component_card(component, cx),
            ));
        }
        rows
    }

    /// A component's page: breadcrumb, Overview · Versions, the cards that
    /// say what is wrong, and every action the planning API offers.
    fn component_page_rows(&self, id: &ComponentId, cx: &mut Context<Self>) -> Vec<Row> {
        let c = copy(self.locale);
        let a = apps::copy(self.locale);
        let p = palette(cx);
        let Some(component) = self.component_by_id(id) else {
            return vec![self.title_row(shell(self.locale).apps, cx)];
        };
        let mut rows = vec![self.breadcrumb_row(component.name.clone(), cx)];
        rows.extend(self.apps_notices(cx));
        let busy = self.transaction_running();
        let unorderable = self
            .unorderable_check(id)
            .and_then(|check| unorderable_detail(self.locale, check));

        // A blocking drift is why nothing else on this page will work, so it
        // goes above the failure that may have caused it.
        if let Some(notice) = component.drift_notice(self.locale) {
            let adopt = id.clone();
            rows.push(Row::new(
                0.0,
                div().pb(px(12.0)).child(
                    InfoBar::new("drift", Severity::Warning, notice.title)
                        .message(format!("{} {}", notice.detail, notice.consequence))
                        .locale(self.locale.words())
                        .action(
                            Button::standard("drift-adopt", c.adopt_host_state)
                                .disabled(busy)
                                .on_click(Self::act(cx, move |this, _, cx| {
                                    this.set_dialog(Some(Dialog::Adopt { id: adopt.clone() }), cx)
                                })),
                        ),
                ),
            ));
        }
        if let Some(detail) = unorderable.clone() {
            rows.push(Row::new(
                0.0,
                div().pb(px(12.0)).child(
                    InfoBar::new(
                        "unorderable",
                        Severity::Warning,
                        c.check_unorderable_version,
                    )
                    .message(detail)
                    .locale(self.locale.words()),
                ),
            ));
        }
        let trailing = self.row_trailing(&component);
        if let Some(failure) = &component.failure {
            let (key, detail) = failure.evidence_parts();
            let mut details = vec![format!(
                "{}: {}",
                c.failed_stage,
                self.stage_label(failure.stage)
            )];
            details.push(format!("{}: {key}", c.failure_evidence));
            if let Some(detail) = detail {
                details.push(format!("{}: {detail}", c.failure_technical_detail));
            }
            if let Some(recovery) = failure.recovery {
                details.push(format!(
                    "{}: {}",
                    c.recovery_status,
                    match recovery {
                        RecoveryStatus::PartiallyRestored => c.recovery_partial,
                        RecoveryStatus::ManualRecoveryRequired => c.manual_recovery_required,
                        _ => c.restore_available,
                    }
                ));
            }
            let mut bar = ErrorBar::new(
                "last-failure",
                ErrorReport::new(
                    format!(
                        "{}{}{}",
                        c.last_install_failed,
                        c.label_separator,
                        self.failure_sentence(&component)
                    ),
                    details.join("\n"),
                ),
            )
            .locale(self.locale.words());
            if let Trailing::Failed { .. } = trailing {
                bar = bar.next_step(self.trailing_element(&component, &trailing, cx));
            }
            rows.push(Row::new(0.0, div().pb(px(12.0)).child(bar)));
        }

        // The header card: what it is, and the one thing at its edge.
        let header_trailing = match &trailing {
            // The error bar above already offers Retry and Restore.
            Trailing::Failed { .. } if component.failure.is_some() => None,
            Trailing::Failed { .. } => Some(self.trailing_element(&component, &trailing, cx)),
            Trailing::View => None,
            other => Some(self.trailing_element(&component, other, cx)),
        };
        let mut header = SettingsCard::new("component-header", component.name.clone())
            .icon(self.component_glyph(&component))
            .description(self.declared_or_not(&component.summary));
        if let Some(trailing) = header_trailing {
            header = header.content(trailing);
        }
        rows.push(Row::new(0.0, header));

        let tabs = [DetailTab::Overview, DetailTab::Versions];
        rows.push(Row::new(
            GROUP_GAP,
            SelectorBar::new(
                "detail-tabs",
                vec![
                    SelectorItem::new(c.overview_tab),
                    SelectorItem::new(c.versions_tab),
                ],
                tabs.iter().position(|tab| *tab == self.detail_tab),
            )
            .on_select(Self::act_with(cx, move |this, index: usize, _, cx| {
                if let Some(tab) = tabs.get(index) {
                    this.detail_tab = *tab;
                    cx.notify();
                }
            })),
        ));

        let none = |values: &[String]| {
            if values.is_empty() {
                c.none.to_string()
            } else {
                values.join(", ")
            }
        };
        let mut cards: Vec<AnyElement> = Vec::new();
        match self.detail_tab {
            DetailTab::Overview => {
                cards.push(
                    SettingsCard::new("what-it-does", c.what_it_does)
                        .icon(IconName::Info)
                        .description(self.declared_or_not(&component.detail))
                        .into_any_element(),
                );
                let pill = Self::status_pill(self.locale, component.state, false);
                cards.push(
                    SettingsCard::new("component-status", a.status)
                        .icon(IconName::CircleCheck)
                        .content(label(pill.label, BODY, p.text_secondary))
                        .into_any_element(),
                );
                cards.push(
                    SettingsCard::new("component-kind", a.kind)
                        .icon(IconName::Inbox)
                        .content(label(
                            Self::kind_pill(self.locale, component.kind).label,
                            BODY,
                            p.text_secondary,
                        ))
                        .into_any_element(),
                );
                cards.push(
                    SettingsCard::new("component-replaces", c.replaces_label)
                        .icon(IconName::Replace)
                        .description(none(&component.replaces))
                        .into_any_element(),
                );
                cards.push(
                    SettingsCard::new("component-enhances", c.enhances_label)
                        .icon(IconName::Plus)
                        .description(none(&component.enhances))
                        .into_any_element(),
                );
                // What happens after the update, only while one is waiting.
                if component.state == ComponentStatus::UpdateAvailable
                    && let Some(effect) = self.restart_requirement_label(
                        DesiredOperation::Update,
                        component.restart_requirement,
                    )
                {
                    cards.push(
                        SettingsCard::new("component-restart", c.restart_requirement)
                            .icon(IconName::Redo)
                            .description(effect)
                            .into_any_element(),
                    );
                }
                let mut checks = SettingsExpander::new("health-checks", a.health_checks, false)
                    .icon(IconName::CircleCheck);
                if component.health_checks.is_empty() {
                    checks = checks.description(a.health_checks_none);
                }
                for check in &component.health_checks {
                    checks = checks.row(SettingsRow::new(check.clone()));
                }
                cards.push(checks.into_any_element());
                let mut files = SettingsExpander::new("files-touched", c.files_touched, false)
                    .icon(IconName::Folder)
                    .description(c.no_privileged_actions);
                for path in &component.paths {
                    files = files.row(SettingsRow::new(path.clone()));
                }
                cards.push(files.into_any_element());
                if component.declares_defaults {
                    let defaults = id.clone();
                    cards.push(
                        SettingsCard::new("component-defaults", a.default_apps)
                            .icon(IconName::Star)
                            .description(a.default_apps_description)
                            .on_click(Self::act(cx, move |this, _, cx| {
                                this.navigate(Page::DefaultsComponent(defaults.clone()), cx)
                            }))
                            .into_any_element(),
                    );
                }
            }
            DetailTab::Versions => {
                let mut installed = SettingsCard::new("installed-version", c.current_version)
                    .icon(IconName::Inbox)
                    .content(label(
                        component.installed_label(c.not_installed),
                        BODY,
                        p.text_secondary,
                    ));
                if component.installed_outside_manager() {
                    installed = installed.description(c.installed_outside_manager);
                }
                cards.push(installed.into_any_element());
                cards.push(
                    SettingsCard::new("available-version", c.available_version)
                        .icon(IconName::ArrowDown)
                        .content(label(
                            component.available_version.clone(),
                            BODY,
                            p.text_secondary,
                        ))
                        .into_any_element(),
                );
                let mut notes = SettingsExpander::new("release-notes", c.release_notes, true)
                    .icon(IconName::BookOpen);
                if component.release_notes.is_empty() {
                    notes = notes.description(c.no_release_notes);
                }
                for note in &component.release_notes {
                    notes = notes.row(SettingsRow::new(note.clone()));
                }
                cards.push(notes.into_any_element());
            }
        }
        for (index, card) in cards.into_iter().enumerate() {
            rows.push(Row::new(
                if index == 0 { FIRST_CARD_GAP } else { CARD_GAP },
                card,
            ));
        }

        // Manage: every action the planning API offers for it, and nothing
        // it would refuse. The install or update is on the header card.
        let installed = component.installed_version.is_some();
        let plannable = unorderable.is_none()
            && component.drift.is_none()
            && component.state != ComponentStatus::Incompatible;
        let mut actions: Vec<(DesiredOperation, &'static str, IconName)> = Vec::new();
        if plannable && installed {
            if component.enabled {
                actions.push((DesiredOperation::Disable, c.disable, IconName::CircleX));
            } else {
                actions.push((DesiredOperation::Enable, c.enable, IconName::CircleCheck));
            }
            actions.push((DesiredOperation::Verify, c.retry_check, IconName::Search));
        }
        if plannable && component.restore_available {
            actions.push((
                DesiredOperation::Restore,
                c.restore_previous,
                IconName::Undo,
            ));
        }
        let is_self = is_self_component(id);
        if plannable && installed && !is_self {
            actions.push((DesiredOperation::Remove, c.remove, IconName::Delete));
        }
        if !actions.is_empty() || (installed && is_self) {
            rows.push(self.group_header(a.actions_header, cx));
        }
        let mut gap = FIRST_CARD_GAP;
        for (operation, header, icon) in actions {
            let target = id.clone();
            rows.push(Row::new(
                gap,
                SettingsCard::new(
                    ElementId::Name(format!("manage-{operation:?}").into()),
                    header,
                )
                .icon(icon)
                .content(
                    Button::standard(
                        ElementId::Name(format!("manage-{operation:?}-button").into()),
                        apps::action_label(self.locale, operation),
                    )
                    .disabled(busy)
                    .on_click(Self::act(cx, move |this, _, cx| {
                        this.request_operation(&target, operation, cx)
                    })),
                ),
            ));
            gap = CARD_GAP;
        }
        // The manager is a component in its own catalog, and the one whose
        // removal it cannot carry out. Saying why beats an absent action.
        if installed && is_self {
            rows.push(Row::new(
                gap,
                SettingsCard::new("cannot-remove-self", c.remove)
                    .icon(IconName::Delete)
                    .description(c.cannot_remove_self)
                    .disabled(true),
            ));
        }
        rows
    }

    /// Apps › Updates: what the check found, Update all, and each update
    /// with its own progress, result, and release notes.
    fn updates_rows(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let c = copy(self.locale);
        let a = apps::copy(self.locale);
        let p = palette(cx);
        let mut rows = vec![self.breadcrumb_row(shell(self.locale).apps_updates.to_string(), cx)];
        rows.extend(self.apps_notices(cx));

        let check = self.update_check();
        let line = CatalogLine::present(self.locale, &self.catalog_status, now_unix_seconds());
        let (title, failure) = match &check {
            UpdateCheck::Running => (c.checking_updates.to_string(), None),
            UpdateCheck::Failed(reason) => (c.update_check_failed.to_string(), Some(*reason)),
            UpdateCheck::Found(sentence) => (sentence.clone(), None),
            UpdateCheck::UpToDate => (c.update_check_up_to_date.to_string(), None),
            UpdateCheck::NoneInCurrentList => (c.update_check_none_in_list.to_string(), None),
        };
        let warning = failure.or(line.warning);
        let count = self.update_plan_count();
        let busy = self.transaction_running();
        let mut buttons = div().flex().items_center().gap(px(8.0)).child(
            Button::standard(
                "check-updates",
                if self.catalog_refreshing {
                    c.catalog_refreshing
                } else {
                    c.check_updates
                },
            )
            .disabled(self.catalog_refreshing)
            .on_click(Self::act(cx, |this, _, cx| this.check_for_updates(cx))),
        );
        // The one accent button, and only when there is something to install
        // (audit M4).
        if count > 0 && !busy {
            buttons = buttons.child(
                Button::accent("update-all", a.update_all)
                    .disabled(self.catalog_refreshing)
                    .on_click(Self::act(cx, |this, _, cx| this.start_update_all(cx))),
            );
        }
        let mut summary = SettingsCard::new("update-summary", title).description(line.age.clone());
        summary = match (&check, warning) {
            (UpdateCheck::Running, _) => summary
                .leading(ProgressRing::new("check-ring", Progress::Indeterminate).size(px(20.0))),
            (_, Some(_)) => summary.status(Severity::Warning),
            (UpdateCheck::Found(_), None) => summary.icon(IconName::ArrowDown),
            _ => summary.status(Severity::Success),
        };
        // While a change runs, its card above says what is happening; a
        // count of what is left would read as "nothing to update" for the
        // components it is updating right now.
        if !busy {
            rows.push(Row::new(0.0, summary.content(buttons)));
            if let Some(warning) = warning {
                rows.push(Row::new(CARD_GAP, label(warning, CAPTION, p.caution)));
            }
            if let Some(rejected) = line.rejected_line(self.locale) {
                rows.push(Row::new(0.0, label(rejected, CAPTION, p.text_secondary)));
            }
        }

        // The rows: every waiting update, and whatever the running or the
        // finished change touched, so its progress and result stay in view.
        let plan = self.update_all_plan();
        let running: Vec<ComponentId> = self
            .run_facts()
            .map(|run| {
                run.steps
                    .iter()
                    .map(|step| step.component.clone())
                    .collect()
            })
            .unwrap_or_default();
        let finished: Vec<ComponentId> = self
            .finished_steps()
            .into_iter()
            .map(|step| step.component)
            .collect();
        let shown: Vec<ComponentInfo> = self
            .components()
            .into_iter()
            .filter(|component| {
                component.state == ComponentStatus::UpdateAvailable
                    || running.contains(&component.core_id)
                    || finished.contains(&component.core_id)
            })
            .collect();
        let updates_manager = shown
            .iter()
            .any(|component| is_self_component(&component.core_id))
            && (count > 0 || !running.is_empty());
        if updates_manager {
            rows.push(Row::new(
                GROUP_GAP,
                InfoBar::new("self-update", Severity::Informational, a.self_update_title)
                    .message(a.self_update_detail)
                    .locale(self.locale.words()),
            ));
        }
        rows.push(self.group_header(a.available_updates, cx));
        if shown.is_empty() {
            rows.push(Row::new(
                FIRST_CARD_GAP,
                label(a.no_updates, BODY, p.text_secondary),
            ));
        }
        for (index, component) in shown.iter().enumerate() {
            let trailing = self.row_trailing(component);
            let step = self
                .run_facts()
                .and_then(|run| {
                    run.steps
                        .iter()
                        .find(|step| step.component == component.core_id)
                        .cloned()
                })
                .or_else(|| {
                    plan.as_ref().and_then(|plan| {
                        plan.steps()
                            .iter()
                            .find(|step| step.component == component.core_id)
                            .cloned()
                    })
                })
                .or_else(|| {
                    self.finished_steps()
                        .into_iter()
                        .find(|step| step.component == component.core_id)
                });
            let description = match (&trailing, &step) {
                (Trailing::Failed { .. }, _) => self.failure_sentence(component),
                (_, Some(step)) => apps::update_facts(
                    self.locale,
                    step,
                    self.restart_requirement_label(step.operation, step.restart_requirement),
                ),
                _ => component.version_label(c.not_installed),
            };
            let id = component.core_id.clone();
            let open = self.notes_open.contains(&id);
            let mut row =
                SettingsExpander::new(element_id("update", &id), component.name.clone(), open)
                    .icon(self.component_glyph(component))
                    .description(description)
                    .content(self.trailing_element(component, &trailing, cx))
                    .on_toggle(Self::act_with(cx, move |this, open: bool, _, cx| {
                        if open {
                            this.notes_open.insert(id.clone());
                        } else {
                            this.notes_open.remove(&id);
                        }
                        cx.notify();
                    }));
            if component.release_notes.is_empty() {
                row = row.row(SettingsRow::new(c.no_release_notes));
            }
            for note in &component.release_notes {
                row = row.row(SettingsRow::new(note.clone()));
            }
            rows.push(Row::new(
                if index == 0 { FIRST_CARD_GAP } else { CARD_GAP },
                row,
            ));
        }
        rows
    }

    /// Apps › Health: the interrupted-package card with Repair, the full
    /// check, and each result, with View on a component that failed.
    fn health_rows(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let c = copy(self.locale);
        let a = apps::copy(self.locale);
        let mut rows = vec![self.breadcrumb_row(shell(self.locale).apps_health.to_string(), cx)];
        rows.extend(self.apps_notices(cx));

        let notice = self
            .package_system
            .as_ref()
            .and_then(|state| package_system_notice(self.locale, state));
        let repairing = self.repair_task.is_some();
        if let Some(notice) = &notice {
            let mut message = notice.lines.join("\n");
            message.push('\n');
            message.push_str(notice.consequence);
            message.push('\n');
            message.push_str(c.repair_needs_password);
            rows.push(Row::card(
                0.0,
                pages::apps_card::PACKAGE_SYSTEM,
                div().pb(px(12.0)).child(
                    InfoBar::new("package-system", Severity::Warning, notice.title)
                        .message(message)
                        .locale(self.locale.words())
                        .action(
                            Button::accent(
                                "package-system-repair",
                                if repairing {
                                    c.repairing
                                } else {
                                    c.repair_package_system
                                },
                            )
                            .disabled(repairing || self.transaction_running())
                            .on_click(Self::act(cx, |this, _, cx| this.repair_package_system(cx))),
                        ),
                ),
            ));
        }
        if let Some(result) = &self.repair_result {
            let severity = match result {
                manager_core::repair::RepairResult::Repaired
                | manager_core::repair::RepairResult::NothingToRepair => Severity::Success,
                manager_core::repair::RepairResult::StillInterrupted => Severity::Warning,
                manager_core::repair::RepairResult::Failed { .. } => Severity::Error,
            };
            rows.push(Row::new(
                0.0,
                div().pb(px(12.0)).child(
                    InfoBar::new(
                        "repair-result",
                        severity,
                        repair_result_line(self.locale, result),
                    )
                    .locale(self.locale.words()),
                ),
            ));
        }

        let checks = self.doctor_checks();
        let failed = checks
            .iter()
            .filter(|check| check.status == DoctorCheckStatus::Failed)
            .count();
        let warnings = checks
            .iter()
            .filter(|check| check.status == DoctorCheckStatus::Warning)
            .count();
        let description = match self.doctor_checked_at {
            Some(at) => a.checked_ago.replace(
                "{ago}",
                &crate::home::ago(self.locale, now_unix_seconds(), at),
            ),
            None => c.health_subtitle.to_string(),
        };
        // Repair is the accent button while dpkg needs it; one per screen.
        let run = if notice.is_some() {
            Button::standard("run-doctor", a.run_full_check)
        } else {
            Button::accent("run-doctor", a.run_full_check)
        };
        rows.push(Row::new(
            0.0,
            SettingsCard::new(
                "doctor-summary",
                apps::health_header(self.locale, failed, warnings),
            )
            .status(if failed > 0 {
                Severity::Error
            } else if warnings > 0 {
                Severity::Warning
            } else {
                Severity::Success
            })
            .description(description)
            .content(
                run.disabled(self.transaction_running())
                    .on_click(Self::act(cx, |this, _, cx| this.run_full_check(cx))),
            ),
        ));

        rows.push(self.group_header(a.results_header, cx));
        let mut list = div().flex().flex_col().pt(px(FIRST_CARD_GAP));
        for (index, check) in checks.iter().enumerate() {
            let title = match check.kind {
                DoctorCheckKind::Catalog => c.check_catalog,
                DoctorCheckKind::Compatibility => c.compatibility,
                DoctorCheckKind::ComponentHealth => c.component_health_checks,
                DoctorCheckKind::RestoreData => c.restore_available,
                DoctorCheckKind::HostReconciliation => c.check_host_reconciliation,
                DoctorCheckKind::UnorderableHostVersion => c.check_unorderable_version,
            };
            let (icon, severity) = match check.status {
                DoctorCheckStatus::Passed => (IconName::CircleCheck, Severity::Success),
                DoctorCheckStatus::Warning => (IconName::TriangleAlert, Severity::Warning),
                DoctorCheckStatus::Failed => (IconName::CircleX, Severity::Error),
            };
            let mut caption = self.component_name_for_core(check.component.as_ref());
            if let Some(detail) = unorderable_detail(self.locale, check) {
                caption = format!("{caption} · {detail}");
            }
            let mut row = ListRow::new(ElementId::named_usize("doctor", index), title)
                .status_glyph(icon, severity)
                .caption(caption);
            if check.status != DoctorCheckStatus::Passed
                && let Some(component) = check.component.clone()
            {
                row = row.trailing(
                    Button::standard(ElementId::named_usize("doctor-view", index), a.action_view)
                        .on_click(Self::act(cx, move |this, _, cx| {
                            this.open_component(&component, cx)
                        })),
                );
            }
            list = list.child(row);
        }
        rows.push(Row::new(0.0, list));
        rows
    }

    /// Apps › Activity: the filters, Clear history, and the list.
    fn activity_rows(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let c = copy(self.locale);
        let p = palette(cx);
        let mut rows = vec![self.breadcrumb_row(shell(self.locale).apps_activity.to_string(), cx)];
        rows.extend(self.apps_notices(cx));
        let filters = [
            (ActivityFilter::All, c.all_activity),
            (ActivityFilter::Success, c.successful),
            (ActivityFilter::Warning, c.warnings),
            (ActivityFilter::Failure, c.failures),
        ];
        let selected = filters
            .iter()
            .position(|(filter, _)| *filter == self.activity_filter);
        let items = filters
            .iter()
            .map(|(_, label)| SelectorItem::new(*label))
            .collect();
        rows.push(Row::new(
            0.0,
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(
                    SelectorBar::new("activity-filter", items, selected).on_select(Self::act_with(
                        cx,
                        move |this, index: usize, _, cx| {
                            if let Some((filter, _)) = filters.get(index) {
                                this.activity_filter = *filter;
                                cx.notify();
                            }
                        },
                    )),
                )
                .child(
                    Button::standard("activity-clear", c.clear_history)
                        .disabled(self.state.activity.is_empty())
                        .on_click(Self::act(cx, |this, _, cx| {
                            this.set_dialog(Some(Dialog::ClearActivity), cx)
                        })),
                ),
        ));
        let entries: Vec<_> = self
            .state
            .activity
            .iter()
            .rev()
            .filter(|entry| self.activity_matches(entry))
            .collect();
        if entries.is_empty() {
            rows.push(Row::new(
                FIRST_CARD_GAP,
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(label(
                        c.no_activity_title,
                        better_ui::fluent::BODY_STRONG,
                        p.text_primary,
                    ))
                    .child(label(c.no_activity_subtitle, BODY, p.text_secondary)),
            ));
            return rows;
        }
        let now = now_unix_seconds();
        let mut list = div().flex().flex_col().pt(px(FIRST_CARD_GAP));
        for entry in entries {
            let (icon, severity) = match crate::home::tone(entry.kind) {
                crate::home::Tone::Success => (IconName::CircleCheck, Severity::Success),
                crate::home::Tone::Warning => (IconName::TriangleAlert, Severity::Warning),
                crate::home::Tone::Failure => (IconName::CircleX, Severity::Error),
                crate::home::Tone::Information => (IconName::Info, Severity::Informational),
            };
            let operation = entry
                .operation
                .map(|operation| self.operation_label(operation))
                .unwrap_or(c.none);
            let detail = entry
                .stage
                .map(|stage| self.stage_label(stage).to_string())
                .unwrap_or_else(|| {
                    crate::app::activity_evidence_text(
                        self.evidence_label(entry.evidence.as_deref(), None),
                        entry.evidence.as_deref(),
                    )
                });
            let caption = match entry.recorded_at_unix_seconds {
                Some(at) => format!("{detail} · {}", crate::home::ago(self.locale, now, at)),
                None => detail,
            };
            list = list.child(
                ListRow::new(
                    ElementId::named_usize("activity", entry.sequence as usize),
                    format!(
                        "{} · {}",
                        self.component_name_for_core(entry.component.as_ref()),
                        operation
                    ),
                )
                .status_glyph(icon, severity)
                .caption(caption),
            );
        }
        rows.push(Row::new(0.0, list));
        rows
    }

    /// The confirmation dialog Apps is showing, drawn over the window.
    pub(crate) fn apps_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let c = copy(self.locale);
        let a = apps::copy(self.locale);
        let p = palette(cx);
        let cancel: SharedString = c.cancel.into();
        let buttons = |primary: &str| DialogButtons {
            primary: Some(primary.to_string().into()),
            secondary: None,
            close: Some(cancel.clone()),
            default: DefaultButton::Close,
        };
        let dialog = self.dialog.clone()?;
        let element = match dialog {
            Dialog::Remove { id, delete_data } => {
                let name = self.plan_component_name(&id);
                let offers_box = self
                    .component_by_id(&id)
                    .is_some_and(|component| component.has_personal_data);
                let confirm = id.clone();
                let mut dialog = ContentDialog::new(
                    "remove-dialog",
                    a.remove_title.replace("{component}", &name),
                )
                .buttons(buttons(a.action_remove))
                .on_result(Self::act_with(
                    cx,
                    move |this, result: DialogResult, _, cx| {
                        if result == DialogResult::Primary {
                            this.confirm_remove(&confirm, delete_data && offers_box, cx);
                        } else {
                            this.set_dialog(None, cx);
                        }
                    },
                ))
                .child(label(a.remove_body, BODY, p.text_primary));
                if offers_box {
                    let toggle = id.clone();
                    dialog = dialog.child(
                        div()
                            .pt(px(12.0))
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(
                                CheckBox::new("delete-data", CheckState::from_bool(delete_data))
                                    .label(a.delete_data)
                                    .on_change(Self::act_with(
                                        cx,
                                        move |this, state: CheckState, _, cx| {
                                            this.set_dialog(
                                                Some(Dialog::Remove {
                                                    id: toggle.clone(),
                                                    delete_data: state == CheckState::Checked,
                                                }),
                                                cx,
                                            )
                                        },
                                    )),
                            )
                            .child(div().pl(px(28.0)).child(label(
                                a.delete_data_detail,
                                CAPTION,
                                p.text_secondary,
                            ))),
                    );
                }
                dialog.into_any_element()
            }
            Dialog::Restore { id } => {
                let name = self.plan_component_name(&id);
                let record = self.state.component(&id);
                let title = match record
                    .and_then(|record| record.restore_snapshot.as_ref())
                    .and_then(|snapshot| snapshot.installed_version.clone())
                {
                    Some(version) => a
                        .restore_title
                        .replace("{component}", &name)
                        .replace("{version}", &version),
                    None => a.restore_title_unknown.replace("{component}", &name),
                };
                let current = record
                    .and_then(|record| record.installed_version.clone())
                    .unwrap_or_else(|| c.not_installed.to_string());
                let confirm = id.clone();
                ContentDialog::new("restore-dialog", title)
                    .buttons(buttons(a.action_restore))
                    .on_result(Self::act_with(
                        cx,
                        move |this, result: DialogResult, _, cx| {
                            this.set_dialog(None, cx);
                            if result == DialogResult::Primary {
                                this.start_operation(&confirm, DesiredOperation::Restore, cx);
                            }
                        },
                    ))
                    .child(label(
                        a.restore_body.replace("{current}", &current),
                        BODY,
                        p.text_primary,
                    ))
                    .into_any_element()
            }
            Dialog::Adopt { id } => {
                let confirm = id.clone();
                ContentDialog::new("adopt-dialog", c.adopt_host_state)
                    .buttons(buttons(c.adopt_host_state_confirm_action))
                    .on_result(Self::act_with(
                        cx,
                        move |this, result: DialogResult, _, cx| {
                            if result == DialogResult::Primary {
                                this.adopt_host_state(&confirm, cx);
                            } else {
                                this.set_dialog(None, cx);
                            }
                        },
                    ))
                    .child(label(c.adopt_host_state_confirm, BODY, p.text_primary))
                    .into_any_element()
            }
            Dialog::ClearActivity => ContentDialog::new("clear-dialog", a.clear_title)
                .buttons(buttons(a.clear))
                .on_result(Self::act_with(cx, |this, result: DialogResult, _, cx| {
                    this.set_dialog(None, cx);
                    if result == DialogResult::Primary {
                        this.clear_activity(cx);
                    }
                }))
                .child(label(a.clear_body, BODY, p.text_primary))
                .into_any_element(),
        };
        Some(element)
    }
}

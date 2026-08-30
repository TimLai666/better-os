//! The window's state and everything it can do.
//!
//! Three invariants hold here and are worth naming.
//!
//! Sampling never runs on the render thread. A background task owns the
//! collectors and posts finished rounds; the window only adopts them.
//!
//! Pausing the display stops adoption, not collection.
//!
//! The window never constructs a signal. It asks a
//! [`monitor_core::ProcessController`] for an action's availability before
//! offering it, and hands the action to the controller to carry out.

use gpui::*;
use gpui_component::{Theme, ThemeMode, input::InputState, table::TableState};
use monitor_actions_linux::LinuxProcessController;
use monitor_collectors_linux::ProcessPrivacy;
use monitor_core::{
    ActionError, ActionOutcome, ActionRefusal, CollectorReport, ProcessAction, ProcessController,
};
use monitor_ipc::{HistoryDocument, StatusDocument};
use monitor_store::{Incident, Inventory, InventoryDiff};
use monitor_views::grouping::GroupingPrecedence;
use monitor_views::{AppsModel, OverviewModel, ProcessFacts};

use crate::i18n::Locale;
use crate::link::{self, DEFAULT_HISTORY_SECONDS, LinkRequest, LinkUpdate};
use crate::tables::{AppsTableDelegate, ProcessTableDelegate};

/// The pages the navigation offers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Page {
    Overview,
    Apps,
    Processes,
    Cpu,
    Memory,
    Storage,
    Network,
    Gpu,
    Energy,
    History,
    Incidents,
    Inventory,
    Diagnostics,
    Settings,
}

/// What the window is asking the user to confirm.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PendingAction {
    pub(crate) pid: u32,
    pub(crate) name: String,
    pub(crate) action: ProcessAction,
}

/// The result of the last action, kept until the next one replaces it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ActionReport {
    Succeeded { pid: u32, outcome: ActionOutcome },
    Failed { pid: u32, error: ActionError },
    Refused { pid: u32, refusal: ActionRefusal },
}

pub(crate) struct MonitorApp {
    pub(crate) page: Page,
    pub(crate) locale: Locale,
    pub(crate) theme: ThemeMode,
    pub(crate) paused: bool,
    pub(crate) include_command_lines: bool,
    pub(crate) precedence: GroupingPrecedence,

    pub(crate) reports: Vec<CollectorReport>,
    pub(crate) overview: OverviewModel,
    pub(crate) rounds: u64,

    /// Where the numbers are coming from, and why, when it is not the service.
    pub(crate) collection: Collection,
    pub(crate) status: Option<StatusDocument>,
    pub(crate) history: Option<HistoryDocument>,
    pub(crate) history_seconds: u64,
    pub(crate) incidents: Vec<Incident>,
    pub(crate) inventory: Option<Inventory>,
    pub(crate) inventory_captures: u32,
    pub(crate) inventory_diff: Option<InventoryDiff>,
    /// The incident the user marked most recently, kept so the Incidents page
    /// can confirm what was captured rather than only that something was.
    pub(crate) last_marked: Option<u64>,
    pub(crate) last_failure: Option<String>,

    pub(crate) processes: Entity<TableState<ProcessTableDelegate>>,
    pub(crate) apps: Entity<TableState<AppsTableDelegate>>,
    pub(crate) filter: Entity<InputState>,

    pub(crate) selected_pid: Option<u32>,
    pub(crate) pending: Option<PendingAction>,
    pub(crate) last_action: Option<ActionReport>,

    controller: LinuxProcessController,
    link: Option<smol::channel::Sender<LinkRequest>>,
    _pump: Option<Task<()>>,
}

/// How the window is being fed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Collection {
    /// Nothing has answered yet. Not the same as "there is nothing".
    Connecting,
    /// The session service is recording. Closing this window changes nothing.
    Service,
    /// No service, so this window is collecting for itself. Recording stops
    /// when the window closes, and the note says so.
    InProcess { detail: String },
    /// Neither could be started. There is nothing to show and the window says
    /// exactly that rather than drawing empty charts.
    Unavailable { detail: String },
}

impl MonitorApp {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let locale = Locale::System;
        let filter = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::i18n::copy(locale).search_placeholder)
        });
        let processes = cx.new(|cx| {
            TableState::new(ProcessTableDelegate::new(locale), window, cx)
                .col_movable(false)
                .col_selectable(false)
                .row_selectable(true)
        });
        let apps = cx.new(|cx| {
            TableState::new(
                AppsTableDelegate::new(
                    locale,
                    AppsModel::new(Vec::new(), GroupingPrecedence::default()),
                ),
                window,
                cx,
            )
            .col_movable(false)
            .col_selectable(false)
            .row_selectable(true)
        });

        // Better OS is dark-first. `gpui_component::init` installs the light
        // theme, so the choice is applied once the window exists.
        Theme::change(ThemeMode::Dark, Some(window), cx);

        let mut app = Self {
            page: Page::Overview,
            locale,
            theme: ThemeMode::Dark,
            paused: false,
            include_command_lines: false,
            precedence: GroupingPrecedence::default(),
            reports: Vec::new(),
            overview: OverviewModel::from_reports(&[]),
            rounds: 0,
            collection: Collection::Connecting,
            status: None,
            history: None,
            history_seconds: DEFAULT_HISTORY_SECONDS,
            incidents: Vec::new(),
            inventory: None,
            inventory_captures: 0,
            inventory_diff: None,
            last_marked: None,
            last_failure: None,
            processes,
            apps,
            filter,
            selected_pid: None,
            pending: None,
            last_action: None,
            controller: LinuxProcessController::for_current_process(),
            link: None,
            _pump: None,
        };
        app.start_link(cx);
        app.observe_filter(cx);
        app.observe_tables(cx);
        app
    }

    /// Start the bridge to collection.
    ///
    /// The window asks for the service first. Whichever side answers, the
    /// collectors live behind the bridge and never on the render thread, and
    /// the window never reads `/proc` itself.
    fn start_link(&mut self, cx: &mut Context<Self>) {
        let (request_sender, request_receiver) = smol::channel::unbounded::<LinkRequest>();
        let (update_sender, update_receiver) = smol::channel::unbounded::<LinkUpdate>();
        self.link = Some(request_sender);
        link::spawn(request_receiver, update_sender);

        self._pump = Some(cx.spawn(async move |this, cx| {
            while let Ok(update) = update_receiver.recv().await {
                if this
                    .update(cx, |app, cx| app.adopt_update(update, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    /// Ask the bridge for something. Silently does nothing before the bridge
    /// exists, which is only true during construction.
    pub(crate) fn ask(&self, request: LinkRequest) {
        if let Some(link) = &self.link {
            let _ = link.send_blocking(request);
        }
    }

    /// Adopt one update from the bridge.
    ///
    /// While the display is paused a status still arrives and is still
    /// counted, so collection is provably running; it just does not replace
    /// what is on screen.
    fn adopt_update(&mut self, update: LinkUpdate, cx: &mut Context<Self>) {
        match update {
            LinkUpdate::Status(status) => {
                self.rounds = status.rounds_collected;
                if self.collection == Collection::Connecting {
                    self.collection = Collection::Service;
                }
                if !self.paused
                    && let Some(round) = &status.latest_round
                {
                    self.reports = round.clone();
                    self.overview = OverviewModel::from_reports(&self.reports);
                    let processes = self
                        .reports
                        .iter()
                        .find(|report| report.collector.as_str() == "linux.process")
                        .map(ProcessFacts::from_report)
                        .unwrap_or_default();
                    self.apply_processes(processes, cx);
                }
                self.status = Some(*status);
            }
            LinkUpdate::Embedded(detail) => {
                self.collection = Collection::InProcess { detail };
            }
            LinkUpdate::Unavailable(detail) => {
                self.collection = Collection::Unavailable { detail };
            }
            LinkUpdate::History(history) => self.history = Some(*history),
            LinkUpdate::Incidents(incidents) => self.incidents = incidents.incidents,
            LinkUpdate::Inventory(inventory, diff) => {
                self.inventory_captures = inventory.captures;
                self.inventory = inventory.inventory;
                self.inventory_diff = diff.diff;
            }
            LinkUpdate::Marked(document) => {
                self.last_marked = Some(document.incident.id);
                self.last_failure = None;
                // The list the page draws has to include what was just
                // marked, so it is refreshed rather than appended to blindly.
                self.ask(LinkRequest::Incidents);
            }
            LinkUpdate::Export(_) => {}
            LinkUpdate::Failed(detail) => self.last_failure = Some(detail),
        }
        cx.notify();
    }

    fn apply_processes(&mut self, processes: Vec<ProcessFacts>, cx: &mut Context<Self>) {
        self.processes.update(cx, |table, cx| {
            table.delegate_mut().model.update(processes.clone());
            table.refresh(cx);
        });
        self.apps.update(cx, |table, cx| {
            table.delegate_mut().model.update(processes);
            table.delegate_mut().rebuild();
            table.refresh(cx);
        });
    }

    fn observe_filter(&mut self, cx: &mut Context<Self>) {
        cx.subscribe(
            &self.filter.clone(),
            |app, state, _: &gpui_component::input::InputEvent, cx| {
                let text = state.read(cx).value().to_string();
                app.processes.update(cx, |table, cx| {
                    table.delegate_mut().model.set_filter(text.clone());
                    table.refresh(cx);
                });
                app.apps.update(cx, |table, cx| {
                    table.delegate_mut().model.set_filter(text.clone());
                    table.delegate_mut().rebuild();
                    table.refresh(cx);
                });
                cx.notify();
            },
        )
        .detach();
    }

    fn observe_tables(&mut self, cx: &mut Context<Self>) {
        cx.subscribe(
            &self.processes.clone(),
            |app, table, event: &gpui_component::table::TableEvent, cx| {
                if let gpui_component::table::TableEvent::SelectRow(row) = event {
                    app.selected_pid = table
                        .read(cx)
                        .delegate()
                        .process_at(*row)
                        .map(|process| process.pid);
                    // A new selection abandons an unconfirmed action rather
                    // than silently re-aiming it at a different process.
                    app.pending = None;
                    cx.notify();
                }
            },
        )
        .detach();

        cx.subscribe(
            &self.apps.clone(),
            |app, table, event: &gpui_component::table::TableEvent, cx| {
                let gpui_component::table::TableEvent::SelectRow(row) = event else {
                    return;
                };
                let row = *row;
                let clicked = table.read(cx).delegate().row_at(row);
                match clicked {
                    // Clicking a group expands or collapses it.
                    Some(crate::tables::AppsRow::Group { services, index }) => {
                        app.apps.update(cx, |table_state, cx| {
                            let delegate = table_state.delegate_mut();
                            let key = delegate
                                .app_row(services, index)
                                .map(|app_row| app_row.group.key.clone());
                            if let Some(key) = key {
                                delegate.model.toggle_expanded(&key);
                                delegate.rebuild();
                                table_state.refresh(cx);
                            }
                        });
                    }
                    // Clicking a member selects that process, so the detail
                    // panel and its actions are reachable from either table.
                    Some(crate::tables::AppsRow::Member { process }) => {
                        app.selected_pid = app
                            .apps
                            .read(cx)
                            .delegate()
                            .model
                            .process(process)
                            .map(|member| member.pid);
                        app.pending = None;
                    }
                    _ => {}
                }
                cx.notify();
            },
        )
        .detach();
    }

    pub(crate) fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        // The three stored-data pages are pulled on arrival rather than
        // polled, because none of them changes between one frame and the next
        // and a query for fifteen minutes of history is not free.
        match page {
            Page::History => self.ask(LinkRequest::History {
                seconds: self.history_seconds,
            }),
            Page::Incidents => self.ask(LinkRequest::Incidents),
            Page::Inventory => self.ask(LinkRequest::Inventory),
            _ => {}
        }
        cx.notify();
    }

    /// Change how much history the History page shows.
    pub(crate) fn set_history_span(&mut self, seconds: u64, cx: &mut Context<Self>) {
        self.history_seconds = seconds;
        self.ask(LinkRequest::History { seconds });
        cx.notify();
    }

    /// "The system was just slow."
    ///
    /// The window sends the marker and nothing else. What is captured around
    /// it is decided where the readings are, not here.
    pub(crate) fn mark_incident(&mut self, cx: &mut Context<Self>) {
        self.ask(LinkRequest::Mark {
            note: None,
            before_seconds: monitor_store::DEFAULT_WINDOW_BEFORE_SECONDS,
            after_seconds: monitor_store::DEFAULT_WINDOW_AFTER_SECONDS,
            about_pid: self.selected_pid,
        });
        cx.notify();
    }

    /// Whether marking is possible at all right now.
    pub(crate) fn can_mark(&self) -> bool {
        matches!(
            self.collection,
            Collection::Service | Collection::InProcess { .. }
        )
    }

    pub(crate) fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.locale = locale;
        self.processes.update(cx, |table, cx| {
            table.delegate_mut().locale = locale;
            table.delegate_mut().rebuild_columns();
            table.refresh(cx);
        });
        self.apps.update(cx, |table, cx| {
            table.delegate_mut().locale = locale;
            table.delegate_mut().rebuild();
            table.refresh(cx);
        });
        cx.notify();
    }

    pub(crate) fn set_theme(
        &mut self,
        mode: ThemeMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.theme = mode;
        Theme::change(mode, Some(window), cx);
        cx.notify();
    }

    pub(crate) fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        self.paused = !self.paused;
        cx.notify();
    }

    pub(crate) fn toggle_tree_mode(&mut self, cx: &mut Context<Self>) {
        self.processes.update(cx, |table, cx| {
            let model = &mut table.delegate_mut().model;
            let tree = model.tree_mode();
            model.set_tree_mode(!tree);
            table.refresh(cx);
        });
        cx.notify();
    }

    pub(crate) fn tree_mode(&self, cx: &App) -> bool {
        self.processes.read(cx).delegate().model.tree_mode()
    }

    /// Turn command-line collection on or off.
    ///
    /// This changes what is collected, not only what is shown, so the message
    /// goes to the sampler as well as to the table.
    pub(crate) fn set_command_lines(&mut self, include: bool, cx: &mut Context<Self>) {
        self.include_command_lines = include;
        self.ask(LinkRequest::SetPrivacy(ProcessPrivacy {
            include_command_line: include,
        }));
        self.processes.update(cx, |table, cx| {
            table.delegate_mut().model.set_show_command_line(include);
            table.delegate_mut().rebuild_columns();
            table.refresh(cx);
        });
        cx.notify();
    }

    pub(crate) fn selected_process(&self, cx: &App) -> Option<ProcessFacts> {
        let pid = self.selected_pid?;
        self.processes
            .read(cx)
            .delegate()
            .model
            .processes()
            .iter()
            .find(|process| process.pid == pid)
            .cloned()
    }

    /// Whether an action can be offered, and why not when it cannot.
    pub(crate) fn availability(
        &self,
        process: &ProcessFacts,
        action: ProcessAction,
    ) -> monitor_core::ActionAvailability {
        self.controller
            .availability(&process.action_target(), action)
    }

    /// Ask for an action. Destructive ones become a pending confirmation
    /// rather than running; the rest run immediately.
    pub(crate) fn request_action(
        &mut self,
        process: &ProcessFacts,
        action: ProcessAction,
        cx: &mut Context<Self>,
    ) {
        if let Some(refusal) = self.availability(process, action).refusal() {
            self.last_action = Some(ActionReport::Refused {
                pid: process.pid,
                refusal: refusal.clone(),
            });
            cx.notify();
            return;
        }
        if action.requires_confirmation() {
            self.pending = Some(PendingAction {
                pid: process.pid,
                name: process.display_name(),
                action,
            });
            cx.notify();
            return;
        }
        self.run_action(process, action, cx);
    }

    /// Cancel a pending confirmation. Nothing was sent, so nothing is undone.
    pub(crate) fn cancel_pending(&mut self, cx: &mut Context<Self>) {
        self.pending = None;
        cx.notify();
    }

    pub(crate) fn confirm_pending(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let Some(process) = self.selected_process(cx).filter(|p| p.pid == pending.pid) else {
            // The process went away while the question was on screen. Saying
            // so is more useful than sending a signal to whatever now holds
            // that PID.
            self.last_action = Some(ActionReport::Failed {
                pid: pending.pid,
                error: ActionError::ProcessDisappeared { pid: pending.pid },
            });
            cx.notify();
            return;
        };
        self.run_action(&process, pending.action, cx);
    }

    fn run_action(
        &mut self,
        process: &ProcessFacts,
        action: ProcessAction,
        cx: &mut Context<Self>,
    ) {
        let target = process.action_target();
        self.last_action = Some(match self.controller.perform(&target, action) {
            Ok(outcome) => ActionReport::Succeeded {
                pid: process.pid,
                outcome,
            },
            Err(error) => ActionReport::Failed {
                pid: process.pid,
                error,
            },
        });
        cx.notify();
    }
}

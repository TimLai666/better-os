//! The window's state and everything it can do.
//!
//! Three invariants hold here and are worth naming.
//!
//! Sampling never runs on the render thread. A background task owns the
//! collectors and posts finished rounds; the window only adopts them, and the
//! readings and the last minute of history are worked out once per round when
//! a round is adopted, never while drawing.
//!
//! Pausing the display stops adoption, not collection.
//!
//! The window never constructs a signal. It asks a
//! [`monitor_core::ProcessController`] for an action's availability before
//! offering it, and hands the action to the controller to carry out.

use std::path::PathBuf;

use better_ui::fluent::theme::Appearance;
use gpui::*;
use gpui_component::input::InputState;
use monitor_actions_linux::LinuxProcessController;
use monitor_collectors_linux::ProcessPrivacy;
use monitor_core::{
    ActionError, ActionOutcome, ActionRefusal, CollectorReport, ProcessAction, ProcessController,
};
use monitor_ipc::StatusDocument;
use monitor_store::{Incident, Inventory, InventoryDiff};
use monitor_views::grouping::GroupingPrecedence;
use monitor_views::{AppsModel, OverviewModel, ProcessColumn, ProcessFacts, ProcessTableModel};

use crate::charts::{BuiltRound, LiveHistory, Readings, TableSettings};
use crate::i18n::Locale;
use crate::link::{self, DEFAULT_HISTORY_SECONDS, LinkRequest, LinkUpdate};
use crate::startup::{
    AutostartDirs, AutostartEntry, RealSystemctl, ServiceError, ServiceUnit, read_autostart,
    read_services, set_autostart, set_service,
};
use crate::tables::{AppColumn, AppSelection, tree_of};

/// The pages the navigation offers, in the order it offers them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Page {
    Overview,
    Processes,
    Details,
    Performance,
    Startup,
    Services,
    History,
    Events,
    SystemInfo,
    Settings,
}

impl Page {
    /// The navigation's order: the main items, then Settings in the footer.
    pub(crate) const ALL: [Page; 10] = [
        Page::Overview,
        Page::Processes,
        Page::Details,
        Page::Performance,
        Page::Startup,
        Page::Services,
        Page::History,
        Page::Events,
        Page::SystemInfo,
        Page::Settings,
    ];

    pub(crate) fn index(self) -> usize {
        Self::ALL.iter().position(|page| *page == self).unwrap_or(0)
    }

    /// Pages that hold a table which fills the window, and therefore do not
    /// scroll as a whole.
    pub(crate) fn fills(self) -> bool {
        matches!(
            self,
            Page::Processes | Page::Details | Page::Startup | Page::Services
        )
    }
}

/// Which resource the Performance page is showing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Resource {
    Cpu,
    Memory,
    Disk(String),
    Adapter(String),
    /// A graphics card, by its PCI address.
    Gpu(String),
    /// An NPU, by its PCI address.
    Npu(String),
    /// Every temperature, fan, and other hwmon reading.
    Temperatures,
    Battery,
}

/// What an action that ends processes is aimed at.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum EndTarget {
    /// One process.
    Process { pid: u32, name: String },
    /// A process and everything it started, children first.
    Tree { pid: u32, name: String },
    /// Every process of an application.
    App { name: String, pids: Vec<u32> },
}

impl EndTarget {
    pub(crate) fn name(&self) -> &str {
        match self {
            EndTarget::Process { name, .. }
            | EndTarget::Tree { name, .. }
            | EndTarget::App { name, .. } => name,
        }
    }
}

/// A question or a sheet over the page.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Dialog {
    /// "End this?" before anything is sent.
    End(EndTarget),
    /// Everything known about one process.
    Properties(u32),
}

/// The result of the last action, kept until the person closes it or the
/// next one replaces it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ActionReport {
    Succeeded {
        name: String,
        outcome: ActionOutcome,
        count: usize,
    },
    Failed {
        name: String,
        error: ActionError,
    },
    Refused {
        name: String,
        refusal: ActionRefusal,
    },
    /// The file manager could not be opened on a process's folder.
    NoLocation {
        name: String,
    },
    /// A startup app or a service was switched, or could not be.
    Switched {
        name: String,
        enabled: bool,
    },
    /// `detail` is what the system said, shown only under "Show details".
    SwitchFailed {
        name: String,
        detail: String,
    },
    /// Ending was asked for while the list was paused.
    ListPaused {
        name: String,
    },
}

/// The startup apps list.
#[derive(Default)]
pub(crate) struct StartupList {
    pub(crate) entries: Vec<AutostartEntry>,
    pub(crate) loaded: bool,
    pub(crate) selected: Option<String>,
}

/// The services list.
#[derive(Default)]
pub(crate) struct ServiceList {
    pub(crate) services: Vec<ServiceUnit>,
    pub(crate) error: Option<ServiceError>,
    pub(crate) loaded: bool,
    pub(crate) selected: Option<String>,
}

/// An open context menu: what it is about, and where it opened.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OpenMenu {
    pub(crate) target: MenuTarget,
    pub(crate) position: Point<Pixels>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum MenuTarget {
    App(AppSelection),
    Process(u32),
    Startup(String),
    Service(String),
}

pub(crate) struct MonitorApp {
    pub(crate) page: Page,
    pub(crate) locale: Locale,
    /// The person's own choice of the navigation pane's width; `None` means
    /// the window decides from its size.
    pub(crate) compact_choice: Option<bool>,
    pub(crate) paused: bool,
    pub(crate) include_command_lines: bool,
    pub(crate) precedence: GroupingPrecedence,

    pub(crate) reports: Vec<CollectorReport>,
    pub(crate) overview: OverviewModel,
    pub(crate) readings: Readings,
    pub(crate) live: LiveHistory,
    pub(crate) resource: Resource,
    /// The processor chart shows each logical processor rather than the whole.
    pub(crate) per_core: bool,
    pub(crate) rounds: u64,
    pub(crate) now_unix_ms: u64,

    /// Where the numbers are coming from, and why, when it is not the service.
    pub(crate) collection: Collection,
    pub(crate) status: Option<StatusDocument>,
    pub(crate) history: Option<crate::stored::HistoryView>,
    pub(crate) history_seconds: u64,
    pub(crate) incidents: Vec<Incident>,
    pub(crate) inventory: Option<Inventory>,
    pub(crate) inventory_captures: u32,
    pub(crate) inventory_diff: Option<InventoryDiff>,
    /// When the Events page last asked for the incident list, so a slowdown
    /// the service recorded while the page is open shows up on it.
    pub(crate) incidents_asked_at_unix_ms: u64,
    pub(crate) last_failure: Option<String>,

    pub(crate) apps: AppsModel,
    pub(crate) app_sort: (AppColumn, bool),
    pub(crate) app_selection: Option<AppSelection>,
    /// The applications open on the Processes page, kept here so a round
    /// built off the window's thread opens the same ones.
    pub(crate) expanded: Vec<String>,
    round_generation: u64,
    applied_generation: u64,
    pub(crate) processes: ProcessTableModel,
    pub(crate) selected_pid: Option<u32>,
    pub(crate) filter: Entity<InputState>,

    pub(crate) startup: StartupList,
    pub(crate) services: ServiceList,
    pub(crate) startup_sort: Option<better_ui::fluent::list::SortState>,
    pub(crate) services_sort: Option<better_ui::fluent::list::SortState>,
    /// Which settings expanders are open.
    pub(crate) grouping_open: bool,
    pub(crate) sources_open: bool,
    /// The Events page's rules expander, once the person opened or closed
    /// it. Until then it is open while there is nothing recorded to show.
    pub(crate) slowdown_rules_open: Option<bool>,

    pub(crate) dialog: Option<Dialog>,
    pub(crate) menu: Option<OpenMenu>,
    pub(crate) menu_state: Entity<better_ui::fluent::menu::MenuOpen>,
    pub(crate) last_action: Option<ActionReport>,

    controller: LinuxProcessController,
    link: Option<smol::channel::Sender<LinkRequest>>,
    _pump: Option<Task<()>>,
    /// Watches for an update and restarts into it (ticket 105).
    pub(crate) update_restart: better_ui::restart::UpdateRestart,
}

/// The window's application id, which the desktop entry is named after.
pub(crate) const APP_ID: &str = "io.betteros.Monitor";

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

/// Where the language choice is kept, beside the appearance.
/// How often the Events page asks again while a slowdown it shows is still
/// going on, so its length and its end appear without leaving the page.
pub(crate) const ONGOING_REFRESH_MS: u64 = 10_000;

/// Whether the Events page's list is behind the service: it holds a different
/// number of incidents than the store does, or it shows a slowdown still
/// going on and has not asked for a while.
pub(crate) fn incidents_are_stale(app: &MonitorApp, status: &StatusDocument) -> bool {
    list_is_stale(
        &app.incidents,
        status.store.incidents,
        app.incidents_asked_at_unix_ms,
        status.now_unix_ms,
    )
}

pub(crate) fn list_is_stale(
    shown: &[Incident],
    stored: u64,
    asked_at_unix_ms: u64,
    now_unix_ms: u64,
) -> bool {
    let ongoing = shown.iter().any(|incident| {
        incident
            .detection
            .as_ref()
            .is_some_and(|detection| detection.ongoing)
    });
    shown.len() as u64 != stored
        || ongoing && now_unix_ms.saturating_sub(asked_at_unix_ms) >= ONGOING_REFRESH_MS
}

pub(crate) fn language_path() -> Option<PathBuf> {
    Appearance::default_path("monitor").map(|path| path.with_file_name("language"))
}

impl MonitorApp {
    pub(crate) fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        handoff: Option<&better_ui::restart::Handoff>,
    ) -> Self {
        let locale = language_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| Locale::parse(&text))
            .unwrap_or(Locale::System);
        let appearance = Appearance::default_path("monitor")
            .map(|path| Appearance::load(&path))
            .unwrap_or_default();
        appearance.apply_and_follow(window, cx);
        let filter = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::i18n::copy(locale).search_placeholder)
        });
        let menu_state = better_ui::fluent::menu::new_state(cx);

        let mut processes = ProcessTableModel::new(Vec::new());
        processes.set_sort(
            ProcessColumn::CpuUtilization,
            monitor_views::SortDirection::Descending,
        );

        let mut app = Self {
            page: Page::Overview,
            locale,
            compact_choice: None,
            paused: false,
            include_command_lines: false,
            precedence: GroupingPrecedence::default(),
            reports: Vec::new(),
            overview: OverviewModel::from_reports(&[]),
            readings: Readings::default(),
            live: LiveHistory::default(),
            resource: Resource::Cpu,
            per_core: false,
            rounds: 0,
            now_unix_ms: 0,
            collection: Collection::Connecting,
            status: None,
            history: None,
            history_seconds: DEFAULT_HISTORY_SECONDS,
            incidents: Vec::new(),
            inventory: None,
            inventory_captures: 0,
            inventory_diff: None,
            incidents_asked_at_unix_ms: 0,
            last_failure: None,
            apps: AppsModel::new(Vec::new(), GroupingPrecedence::default()),
            app_sort: (AppColumn::Cpu, false),
            app_selection: None,
            expanded: Vec::new(),
            round_generation: 0,
            applied_generation: 0,
            processes,
            selected_pid: None,
            filter,
            startup: StartupList::default(),
            services: ServiceList::default(),
            startup_sort: None,
            services_sort: None,
            grouping_open: false,
            sources_open: false,
            slowdown_rules_open: None,
            dialog: None,
            menu: None,
            menu_state,
            last_action: None,
            controller: LinuxProcessController::for_current_process(),
            link: None,
            _pump: None,
            update_restart: better_ui::restart::UpdateRestart::new(
                APP_ID,
                "Better Monitor",
                "better-monitor",
                env!("CARGO_PKG_VERSION"),
                handoff,
                window,
                cx,
            ),
        };
        if let Some(view) = handoff.and_then(|h| h.state::<crate::restart::MonitorView>()) {
            app.restore_view(view, window, cx);
        }
        app.start_link(cx);
        app.observe_filter(cx);
        // The inventory names the graphics card and the processor model, so
        // it is asked for once at the start rather than only on its page.
        app.ask(LinkRequest::Inventory);
        app
    }

    /// Start the bridge to collection.
    ///
    /// The window asks for the service first. Whichever side answers, the
    /// collectors live behind the bridge and never on the render thread, and
    /// the window never reads `/proc` for a reading itself.
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
                    self.now_unix_ms = status.now_unix_ms;
                    self.adopt_round(round.clone(), cx);
                }
                if self.page == Page::Events && incidents_are_stale(self, &status) {
                    self.incidents_asked_at_unix_ms = status.now_unix_ms;
                    self.ask(LinkRequest::Incidents);
                }
                self.status = Some(*status);
            }
            LinkUpdate::Embedded(detail) => {
                self.collection = Collection::InProcess { detail };
            }
            LinkUpdate::Connected => self.collection = Collection::Service,
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
            LinkUpdate::Export(_) => {}
            LinkUpdate::Failed(detail) => self.last_failure = Some(detail),
        }
        cx.notify();
    }

    /// What the tables are showing now, for a round built elsewhere.
    pub(crate) fn table_settings(&self) -> TableSettings {
        TableSettings {
            sort: self.processes.sort(),
            filter: self.processes.filter().to_string(),
            tree: self.processes.tree_mode(),
            command_lines: self.include_command_lines,
            precedence: self.precedence.clone(),
            expanded: self.expanded.clone(),
        }
    }

    /// Turn one round into the overview, the readings, and both tables on a
    /// background thread, then adopt the result in one step. A round that
    /// finishes after a newer one has been adopted is dropped.
    pub(crate) fn adopt_round(&mut self, round: Vec<CollectorReport>, cx: &mut Context<Self>) {
        self.round_generation += 1;
        let generation = self.round_generation;
        let settings = self.table_settings();
        let now = self.now_unix_ms as f64 / 1000.0;
        cx.spawn(async move |this, cx| {
            let built = cx
                .background_spawn(async move { BuiltRound::build(round, now, settings) })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.apply_round(generation, built);
                cx.notify();
            });
        })
        .detach();
    }

    /// Adopt a round built off the window's thread. Settings the person
    /// changed while it was being built are applied on top.
    pub(crate) fn apply_round(&mut self, generation: u64, built: BuiltRound) {
        if generation <= self.applied_generation || self.paused {
            return;
        }
        self.applied_generation = generation;
        let current = self.table_settings();
        let BuiltRound {
            reports,
            overview,
            readings,
            mut processes,
            mut apps,
            settings,
        } = built;
        if settings != current {
            current.apply_processes(&mut processes);
            current.apply_apps(&mut apps);
        }
        self.reports = reports;
        self.overview = overview;
        self.readings = readings;
        self.live.record(&self.readings);
        self.processes = processes;
        self.apps = apps;
        // A group that is gone stops being remembered as open.
        let apps = &self.apps;
        self.expanded.retain(|key| apps.is_expanded(key));
        // A resource that went away (a disk unplugged) is not left selected
        // on a page that can no longer draw it.
        match &self.resource {
            Resource::Disk(name) if !self.readings.disks.iter().any(|d| &d.name == name) => {
                self.resource = Resource::Cpu;
            }
            Resource::Adapter(name) if !self.readings.adapters.iter().any(|a| &a.name == name) => {
                self.resource = Resource::Cpu;
            }
            Resource::Gpu(key) if !self.readings.gpus.iter().any(|g| &g.key == key) => {
                self.resource = Resource::Cpu;
            }
            Resource::Npu(key) if !self.readings.npus.iter().any(|n| &n.key == key) => {
                self.resource = Resource::Cpu;
            }
            _ => {}
        }
    }

    fn observe_filter(&mut self, cx: &mut Context<Self>) {
        cx.subscribe(
            &self.filter.clone(),
            |app, state, _: &gpui_component::input::InputEvent, cx| {
                let text = state.read(cx).value().to_string();
                app.processes.set_filter(text.clone());
                app.apps.set_filter(text);
                cx.notify();
            },
        )
        .detach();
    }

    pub(crate) fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        self.menu = None;
        // The stored-data pages are pulled on arrival rather than polled,
        // because none of them changes between one frame and the next and a
        // query for fifteen minutes of history is not free.
        match page {
            Page::History => self.ask(LinkRequest::History {
                seconds: self.history_seconds,
            }),
            Page::Events => {
                self.incidents_asked_at_unix_ms = self.now_unix_ms;
                self.ask(LinkRequest::Incidents);
            }
            Page::SystemInfo | Page::Performance => self.ask(LinkRequest::Inventory),
            Page::Startup => self.reload_startup(cx),
            Page::Services => self.reload_services(cx),
            _ => {}
        }
        cx.notify();
    }

    pub(crate) fn show_resource(&mut self, resource: Resource, cx: &mut Context<Self>) {
        self.resource = resource;
        self.navigate(Page::Performance, cx);
    }

    /// Change how much history the History page shows.
    pub(crate) fn set_history_span(&mut self, seconds: u64, cx: &mut Context<Self>) {
        self.history_seconds = seconds;
        self.ask(LinkRequest::History { seconds });
        cx.notify();
    }

    /// Whether something is recording, and so recording slowdowns, right
    /// now.
    pub(crate) fn is_recording(&self) -> bool {
        matches!(
            self.collection,
            Collection::Service | Collection::InProcess { .. }
        )
    }

    pub(crate) fn set_locale(
        &mut self,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.locale = locale;
        if let Some(path) = language_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, format!("{}\n", locale.as_str()));
        }
        let placeholder = crate::i18n::copy(locale).search_placeholder;
        self.filter.update(cx, |state, cx| {
            state.set_placeholder(placeholder, window, cx);
        });
        if self.page == Page::Startup {
            self.reload_startup(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_appearance(
        &mut self,
        appearance: Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        appearance.apply_and_follow(window, cx);
        if let Some(path) = Appearance::default_path("monitor") {
            let _ = appearance.save(&path);
        }
        cx.notify();
    }

    pub(crate) fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        self.paused = !self.paused;
        cx.notify();
    }

    pub(crate) fn toggle_tree_mode(&mut self, cx: &mut Context<Self>) {
        let tree = self.processes.tree_mode();
        self.processes.set_tree_mode(!tree);
        cx.notify();
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
        self.processes.set_show_command_line(include);
        cx.notify();
    }

    /// The logical processor count a process's processor time is divided
    /// by, as Task Manager does (100% is every processor). `None` until the
    /// processor has reported, and then nothing is shown rather than a share
    /// of one processor.
    pub(crate) fn processors(&self) -> Option<f64> {
        self.readings
            .cpu
            .logical
            .filter(|count| *count > 0)
            .map(|count| count as f64)
    }

    /// Open or close an application's processes on the Processes page.
    pub(crate) fn toggle_expanded(&mut self, key: &str) {
        self.apps.toggle_expanded(key);
        if self.apps.is_expanded(key) {
            if !self.expanded.iter().any(|k| k == key) {
                self.expanded.push(key.to_string());
            }
        } else {
            self.expanded.retain(|k| k != key);
        }
    }

    pub(crate) fn process(&self, pid: u32) -> Option<&ProcessFacts> {
        self.processes
            .processes()
            .iter()
            .find(|process| process.pid == pid)
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

    /// The first reason none of `pids` can be ended, when not one of them
    /// can; `None` when at least one can.
    pub(crate) fn end_refusal(&self, pids: &[u32]) -> Option<ActionRefusal> {
        let mut first = None;
        for pid in pids {
            let Some(process) = self.process(*pid) else {
                continue;
            };
            // One process that may be ended is enough to offer the action.
            let refusal = self
                .availability(process, ProcessAction::Terminate)
                .refusal()?
                .clone();
            first.get_or_insert(refusal);
        }
        first.or(Some(ActionRefusal::OwnershipUnknown))
    }

    /// The processes an end action would reach, children before parents.
    pub(crate) fn end_pids(&self, target: &EndTarget) -> Vec<u32> {
        match target {
            EndTarget::Process { pid, .. } => vec![*pid],
            EndTarget::Tree { pid, .. } => tree_of(*pid, self.processes.processes()),
            EndTarget::App { pids, .. } => pids.clone(),
        }
    }

    /// Ask to end something: refused at once when nothing it covers may be
    /// ended, otherwise a confirmation.
    pub(crate) fn request_end(&mut self, target: EndTarget, cx: &mut Context<Self>) {
        self.menu = None;
        let pids = self.end_pids(&target);
        if self.paused {
            // A frozen list may name a process that has closed, and its PID
            // may since belong to something else.
            self.last_action = Some(ActionReport::ListPaused {
                name: target.name().to_string(),
            });
        } else if let EndTarget::Tree { .. } = target
            && let Some(refusal) = self.controller.tree_refusal(&pids)
        {
            self.last_action = Some(ActionReport::Refused {
                name: target.name().to_string(),
                refusal,
            });
        } else if let Some(refusal) = self.end_refusal(&pids) {
            self.last_action = Some(ActionReport::Refused {
                name: target.name().to_string(),
                refusal,
            });
        } else {
            self.dialog = Some(Dialog::End(target));
        }
        cx.notify();
    }

    pub(crate) fn close_dialog(&mut self, cx: &mut Context<Self>) {
        self.dialog = None;
        cx.notify();
    }

    /// Carry out a confirmed end. Each process is asked to exit; processes
    /// that may not be ended (another user's, a protected one) are skipped,
    /// and a process that is already gone is not reported as a failure of
    /// the whole.
    pub(crate) fn confirm_end(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::End(target)) = self.dialog.take() else {
            return;
        };
        if self.paused {
            self.last_action = Some(ActionReport::ListPaused {
                name: target.name().to_string(),
            });
            cx.notify();
            return;
        }
        let pids = self.end_pids(&target);
        let mut sent = 0;
        let mut failure = None;
        for pid in &pids {
            let Some(process) = self.process(*pid).cloned() else {
                continue;
            };
            if self
                .availability(&process, ProcessAction::Terminate)
                .refusal()
                .is_some()
            {
                continue;
            }
            match self
                .controller
                .perform(&process.action_target(), ProcessAction::Terminate)
            {
                Ok(_) => sent += 1,
                Err(ActionError::ProcessDisappeared { .. }) => {}
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        let name = target.name().to_string();
        self.last_action = Some(match (sent, failure) {
            (0, Some(error)) => ActionReport::Failed { name, error },
            (0, None) => ActionReport::Failed {
                name,
                error: ActionError::ProcessDisappeared {
                    pid: pids.first().copied().unwrap_or_default(),
                },
            },
            (count, _) => ActionReport::Succeeded {
                name,
                outcome: ActionOutcome::SignalAccepted {
                    signal: monitor_core::SignalKind::Terminate,
                },
                count,
            },
        });
        cx.notify();
    }

    /// Pause, resume, or lower the priority of one process. None of these
    /// loses work, so none asks first.
    pub(crate) fn run_action(&mut self, pid: u32, action: ProcessAction, cx: &mut Context<Self>) {
        self.menu = None;
        let Some(process) = self.process(pid).cloned() else {
            return;
        };
        let name = process.display_name();
        if let Some(refusal) = self.availability(&process, action).refusal() {
            self.last_action = Some(ActionReport::Refused {
                name,
                refusal: refusal.clone(),
            });
            cx.notify();
            return;
        }
        self.last_action = Some(
            match self.controller.perform(&process.action_target(), action) {
                Ok(outcome) => ActionReport::Succeeded {
                    name,
                    outcome,
                    count: 1,
                },
                Err(error) => ActionReport::Failed { name, error },
            },
        );
        cx.notify();
    }

    /// Open the folder a process's program lives in, in the file manager.
    pub(crate) fn open_location(&mut self, pid: u32, cx: &mut Context<Self>) {
        self.menu = None;
        let name = self
            .process(pid)
            .map(ProcessFacts::display_name)
            .unwrap_or_default();
        // The folder is an `OsString` from the link to the program, never
        // converted to text, so a path that is not UTF-8 opens as it is.
        let opened = self
            .controller
            .program_folder(pid)
            .and_then(|folder| {
                std::process::Command::new("xdg-open")
                    .arg(folder)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .ok()
            })
            .map(|mut child| {
                // Reaped on a thread of its own, so a finished `xdg-open` is
                // never left as a zombie under the window.
                std::thread::spawn(move || {
                    let _ = child.wait();
                })
            })
            .is_some();
        if !opened {
            self.last_action = Some(ActionReport::NoLocation { name });
        }
        cx.notify();
    }

    pub(crate) fn show_properties(&mut self, pid: u32, cx: &mut Context<Self>) {
        self.menu = None;
        self.dialog = Some(Dialog::Properties(pid));
        cx.notify();
    }

    pub(crate) fn dismiss_report(&mut self, cx: &mut Context<Self>) {
        self.last_action = None;
        cx.notify();
    }

    pub(crate) fn open_menu(
        &mut self,
        target: MenuTarget,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu = Some(OpenMenu { target, position });
        better_ui::fluent::menu::open_menu(&self.menu_state, Some(0), window, cx);
        cx.notify();
    }

    /// Read the startup apps again, off the window's thread.
    pub(crate) fn reload_startup(&mut self, cx: &mut Context<Self>) {
        let chinese = self.locale.is_chinese();
        cx.spawn(async move |this, cx| {
            let entries = cx
                .background_spawn(async move {
                    AutostartDirs::from_environment()
                        .map(|dirs| read_autostart(&dirs, chinese))
                        .unwrap_or_default()
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.startup.entries = entries;
                app.startup.loaded = true;
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn switch_startup(
        &mut self,
        file_name: String,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        self.menu = None;
        let Some(entry) = self
            .startup
            .entries
            .iter()
            .find(|entry| entry.file_name == file_name)
            .cloned()
        else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let name = entry.name.clone();
            let result = cx
                .background_spawn(async move {
                    let dirs = AutostartDirs::from_environment().ok_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::NotFound, "no home directory")
                    })?;
                    set_autostart(&dirs, &entry, enabled)
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.last_action = Some(match result {
                    Ok(()) => ActionReport::Switched { name, enabled },
                    Err(error) => ActionReport::SwitchFailed {
                        name,
                        detail: error.to_string(),
                    },
                });
                app.reload_startup(cx);
            });
        })
        .detach();
    }

    /// Read the services again, off the window's thread.
    pub(crate) fn reload_services(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { read_services(&RealSystemctl::from_environment()) })
                .await;
            let _ = this.update(cx, |app, cx| {
                match result {
                    Ok(services) => {
                        app.services.services = services;
                        app.services.error = None;
                    }
                    Err(error) => {
                        app.services.services.clear();
                        app.services.error = Some(error);
                    }
                }
                app.services.loaded = true;
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn switch_service(&mut self, unit: String, enabled: bool, cx: &mut Context<Self>) {
        self.menu = None;
        let Some(service) = self
            .services
            .services
            .iter()
            .find(|service| service.unit == unit)
            .cloned()
        else {
            return;
        };
        let locale = self.locale;
        cx.spawn(async move |this, cx| {
            let name = service.title().to_string();
            let result = cx
                .background_spawn(async move {
                    set_service(&RealSystemctl::from_environment(), &service, enabled)
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.last_action = Some(match result {
                    Ok(()) => ActionReport::Switched { name, enabled },
                    Err(error) => ActionReport::SwitchFailed {
                        name,
                        detail: crate::i18n::service_error_detail(&error, locale),
                    },
                });
                app.reload_services(cx);
            });
        })
        .detach();
    }

    pub(crate) fn sort_apps(&mut self, column: AppColumn, cx: &mut Context<Self>) {
        self.app_sort = if self.app_sort.0 == column {
            (column, !self.app_sort.1)
        } else {
            (column, column == AppColumn::Name)
        };
        cx.notify();
    }

    pub(crate) fn sort_processes(&mut self, column: ProcessColumn, cx: &mut Context<Self>) {
        self.processes.sort_by(column);
        cx.notify();
    }
}

impl MonitorApp {
    /// Puts back the view a restart after an update handed over.
    fn restore_view(
        &mut self,
        view: crate::restart::MonitorView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::restart::{column_of, direction, page_of, process_column_of};
        self.page = page_of(view.page);
        self.resource = view.resource.resource();
        self.per_core = view.per_core;
        self.paused = view.paused;
        self.compact_choice = view.compact_choice;
        self.app_sort = (column_of(view.app_sort.0), view.app_sort.1);
        self.app_selection = view.app_selection.map(|key| key.selection());
        self.expanded = view.expanded;
        self.processes.set_sort(
            process_column_of(view.process_sort.0),
            direction(view.process_sort.1),
        );
        self.selected_pid = view.selected_pid;
        let sort = |(column, ascending)| better_ui::fluent::list::SortState { column, ascending };
        self.startup_sort = view.startup_sort.map(sort);
        self.services_sort = view.services_sort.map(sort);
        if let Some(seconds) = view.history_seconds {
            self.history_seconds = seconds;
        }
        self.grouping_open = view.grouping_open;
        self.sources_open = view.sources_open;
        self.slowdown_rules_open = view.slowdown_rules_open;
        if !view.filter.is_empty() {
            self.processes.set_filter(view.filter.clone());
            self.apps.set_filter(view.filter.clone());
            let text = view.filter;
            self.filter
                .update(cx, |state, cx| state.set_value(text, window, cx));
        }
    }
}

impl better_ui::restart::Restartable for MonitorApp {
    type State = crate::restart::MonitorView;

    fn update_restart(&mut self) -> &mut better_ui::restart::UpdateRestart {
        &mut self.update_restart
    }

    /// A question over the page has not been answered yet.
    fn restart_blocked(&self, _cx: &App) -> Option<better_ui::restart::Blocker> {
        self.dialog
            .is_some()
            .then_some(better_ui::restart::Blocker::Dialog)
    }

    fn restart_state(&self, _window: &Window, cx: &App) -> crate::restart::MonitorView {
        use crate::restart::{SelectionKey, ascending, column_index, process_column_index};
        let (process_column, process_direction) = self.processes.sort();
        let sort = |sort: better_ui::fluent::list::SortState| (sort.column, sort.ascending);
        crate::restart::MonitorView {
            page: self.page.index(),
            resource: crate::restart::ResourceKey::of(&self.resource),
            per_core: self.per_core,
            paused: self.paused,
            compact_choice: self.compact_choice,
            filter: self.filter.read(cx).value().to_string(),
            app_sort: (column_index(self.app_sort.0), self.app_sort.1),
            app_selection: self.app_selection.as_ref().map(SelectionKey::of),
            expanded: self.expanded.clone(),
            process_sort: (
                process_column_index(process_column),
                ascending(process_direction),
            ),
            selected_pid: self.selected_pid,
            startup_sort: self.startup_sort.map(sort),
            services_sort: self.services_sort.map(sort),
            history_seconds: Some(self.history_seconds),
            grouping_open: self.grouping_open,
            sources_open: self.sources_open,
            slowdown_rules_open: self.slowdown_rules_open,
        }
    }
}

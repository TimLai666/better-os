use better_core::ComponentId;
use better_ui::fluent::theme::Appearance;
use gpui::*;
use gpui_component::input::{InputEvent, InputState};
use manager_core::ExecutionMode;
use manager_core::catalog::{CatalogStatus, RefreshOutcome, now_unix_seconds};
use manager_core::exec::{CancelToken, RealDriver, RunnerEvent, StageProgress, TransactionRunner};
use manager_core::repair::{self, PackageSystemState, RepairResult};
use manager_core::{
    ActivityKind, ActivityRecord, ComponentFilterPreference, ComponentStatus, DesiredOperation,
    DiskSpaceCheck, DoctorCheck, Manager, ManagerSettings, ManagerState, MockOutcome,
    OperationProgress, OperationStage, PlanStep, StoredLocale, StoredTheme, TransactionPlan,
};
use manager_platform::catalog_fetch::HttpManifestFetcher;
use manager_platform::download::{ArtifactCache, HttpDownloader};
use manager_platform::dpkg::DpkgProbe;
use manager_platform::host::ClientPlatform;
use manager_platform::privileged::DbusPrivilegedExecutor;
use manager_platform::user_units::SystemdUserUnitController;
use manager_platform::{SystemCapabilities, SystemProfile};
use manager_store::{JsonCatalogStore, JsonStore, StateStore, cache_refresh, start_catalog};

use crate::{
    attention::{ComponentFacts, Snapshot},
    deep_link::{self, Request},
    defaults_app::DefaultsState,
    home,
    i18n::{Locale, copy, shell},
    model::{ActivityFilter, CatalogLine, ComponentInfo, DetailTab, Page, UpdateCheck},
    navigation::History,
    search::SearchEntry,
};

/// How this window runs transactions.
///
/// Real is the default: a manager that quietly simulated would tell a user
/// their machine changed when it did not. The demo mode stays available for
/// screenshots and for trying the flow without a privileged service, and says
/// so on screen.
fn default_execution_mode() -> ExecutionMode {
    match std::env::var("BETTER_MANAGER_EXECUTION").as_deref() {
        Ok("mock") | Ok("demo") => ExecutionMode::Mock,
        _ => ExecutionMode::Real,
    }
}

/// The platform this window plans against.
///
/// A demo keeps the fixed mock profile, which is the point of a demo: it has
/// to produce the same screens on any machine and it never changes one. A real
/// window reads the host, because a plan names a package built for exactly one
/// Ubuntu release and one architecture, and the privileged service checks that
/// target against the machine before it applies anything.
pub(crate) fn platform_for(execution: ExecutionMode) -> ClientPlatform {
    match execution {
        ExecutionMode::Mock => ClientPlatform::demo(),
        ExecutionMode::Real => ClientPlatform::host(),
    }
}

/// Builds the manager from whatever the platform reports about the host.
///
/// A host the client cannot identify becomes a state on screen rather than a
/// crash or a silent guess. The manager is still built, from a profile no
/// manifest declares, so every screen keeps working and every plan fails with
/// "this component ships nothing for this host" instead of naming packages
/// built for a release the machine is not running.
pub(crate) fn probe_manager(
    catalog: better_core::manifest::ComponentCatalog,
    platform: &dyn SystemCapabilities,
) -> (Manager, Option<AppError>) {
    match Manager::probe(catalog.clone(), platform) {
        Ok(manager) => (manager, None),
        Err(_) => (
            Manager::new(catalog, SystemProfile::unidentified()),
            Some(AppError::UnsupportedHost),
        ),
    }
}

pub(crate) struct ManagerApp {
    pub(crate) page: Page,
    /// Where the back button goes.
    pub(crate) history: History,
    /// The person closed a wide window's pane with ☰.
    pub(crate) wide_collapsed: bool,
    /// The person opened a narrow window's pane with ☰ or the search button.
    pub(crate) narrow_open: bool,
    /// "Find a setting", in the pane.
    pub(crate) nav_search: Entity<InputState>,
    /// The card a search result asked for, until it has been scrolled to and
    /// focused.
    pub(crate) pending_card: Option<&'static str>,
    /// The card a search result put the keyboard on, while the page shows it.
    pub(crate) focused_card: Option<&'static str>,
    pub(crate) card_focus: FocusHandle,
    /// The window's own focus, so Alt+Left reaches it with nothing focused.
    pub(crate) root_focus: FocusHandle,
    pub(crate) content_scroll: ScrollHandle,
    /// The banner's facts, read when Home is shown.
    pub(crate) device: home::Device,
    /// Whether Settings' About expander is open.
    pub(crate) about_expanded: bool,
    /// Whether the window is drawing dark, read at the start of each frame
    /// for the screens that draw without a context.
    pub(crate) dark: bool,
    /// Set when `keep-awake/until` asked for the "Until a time" dialog, for
    /// ticket 87's page to open and clear.
    pub(crate) until_dialog_requested: bool,
    /// This window's hold on `io.betteros.Manager`; dropping it gives the
    /// name up.
    pub(crate) _bus_owner: Option<deep_link::bus::Owner>,
    pub(crate) locale: Locale,
    pub(crate) search: Entity<InputState>,
    pub(crate) search_query: String,
    pub(crate) detail_tab: DetailTab,
    pub(crate) activity_filter: ActivityFilter,
    pub(crate) manager: Manager,
    pub(crate) state: ManagerState,
    pub(crate) store: JsonStore,
    /// Where the refreshed catalog is kept between sessions.
    pub(crate) catalog_store: JsonCatalogStore,
    /// Where the catalog on screen came from and how far behind it may be.
    pub(crate) catalog_status: CatalogStatus,
    pub(crate) catalog_refreshing: bool,
    /// Whether someone asked this window to check for updates. Until they have,
    /// the Updates screen states nothing about a check that never ran.
    pub(crate) update_check_requested: bool,
    /// The running catalog refresh. Dropping it abandons the fetch, which
    /// changes nothing: a refresh writes only after it has something to write.
    pub(crate) catalog_task: Option<Task<()>>,
    pub(crate) pending_plan: Option<TransactionPlan>,
    /// The transaction that just ran. The result screens describe this rather
    /// than `pending_plan`, so a finished plan stops looking like one still
    /// waiting to be approved.
    pub(crate) finished_plan: Option<TransactionPlan>,
    pub(crate) planning_error: Option<AppError>,
    /// Whether this window simulates transactions or actually performs them.
    pub(crate) execution: ExecutionMode,
    /// The Defaults section's own state, read and changed through
    /// `defaults-core` and nothing else.
    pub(crate) defaults: DefaultsState,
    /// Live progress from a running real transaction.
    pub(crate) transfer: Option<Transfer>,
    /// Set while a real transaction is running. Dropping it stops the work.
    pub(crate) running: Option<Task<()>>,
    pub(crate) cancel: Option<CancelToken>,
    /// The component whose drift card is currently asking "are you sure".
    ///
    /// Adoption throws away the recorded restore point, so it is not something
    /// one click does. The confirmation lives here rather than in a modal
    /// because the sentence a person needs is already on the card they clicked.
    pub(crate) adopt_confirm: Option<ComponentId>,
    /// Better OS packages dpkg holds at a version nothing can order, found
    /// when the window last reconciled. Shown by the doctor, managed by
    /// nothing (ticket 69).
    pub(crate) unorderable: Vec<DoctorCheck>,
    /// What the privileged service last said about dpkg's own record. `None`
    /// until it has been asked, and in a demo window, which never asks.
    pub(crate) package_system: Option<PackageSystemState>,
    /// The running question to the privileged service about dpkg's record.
    pub(crate) package_system_task: Option<Task<()>>,
    /// Set while a repair is waiting on the password prompt or on dpkg.
    pub(crate) repair_task: Option<Task<()>>,
    /// How the last repair this window asked for ended.
    pub(crate) repair_result: Option<RepairResult>,
    /// Set while the startup pass is starting the user units the record says
    /// should run.
    pub(crate) heal_task: Option<Task<()>>,
    pub(crate) _subscriptions: Vec<Subscription>,
    pub(crate) _nav_search_subscription: Subscription,
    /// Takes requests a second `better-manager` sent over the bus.
    pub(crate) _bus_task: Option<Task<()>>,
    /// Redraws once a minute while a page shows how long ago the version
    /// list was checked, so "5 minutes ago" does not stay "just now".
    _age_task: Task<()>,
    /// Watches for an update and restarts into it (ticket 105).
    pub(crate) update_restart: better_ui::restart::UpdateRestart,
    /// The Window attention page (ticket 106).
    pub(crate) window_attention: crate::window_attention_app::WindowAttentionState,
    /// Its "Add an app" search field.
    pub(crate) window_attention_add: Entity<InputState>,
}

/// What a real transaction is currently moving.
#[derive(Clone, Debug)]
pub(crate) struct Transfer {
    pub(crate) component: String,
    pub(crate) received_bytes: u64,
    pub(crate) total_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppError {
    Planning,
    Storage,
    /// This machine is not one Better OS publishes packages for, or could not
    /// be identified. Nothing can be planned for it, and saying so is the only
    /// honest screen: the alternative was a plan aimed at Ubuntu 24.04 amd64
    /// whatever the host actually was.
    UnsupportedHost,
}

impl ManagerApp {
    pub(crate) fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        bus_owner: Option<deep_link::bus::Owner>,
        handoff: Option<&better_ui::restart::Handoff>,
    ) -> Self {
        let store = JsonStore::from_default_path();
        let catalog_store = JsonCatalogStore::from_default_path();
        // The catalog a previous refresh cached, or the compiled-in one. This
        // reads a file and nothing else; fetching happens on a background
        // thread below, so a slow or absent network never delays the window.
        let (catalog, catalog_status) = start_catalog(&catalog_store);
        let execution = default_execution_mode();
        let (manager, host_error) = probe_manager(catalog, &platform_for(execution));
        let (state, planning_error) = match store.load() {
            Ok(outcome) if manager.validate_state(&outcome.state).is_ok() => (
                outcome.state,
                outcome.recovered_corrupt_state.map(|_| AppError::Storage),
            ),
            Ok(_) | Err(_) => (ManagerState::default(), Some(AppError::Storage)),
        };
        // An unidentified host outranks a storage problem: it is why nothing
        // on this machine can be planned at all, and the state file is beside
        // the point until it is resolved.
        let planning_error = host_error.or(planning_error);
        let locale = locale_from_stored(state.settings.locale);
        appearance_of(state.settings.theme).apply_and_follow(window, cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(copy(locale).search));
        let subscription = Self::subscribe_to_search(&search, window, cx);
        let nav_search = Self::nav_search_input(locale, "", window, cx);
        let nav_search_subscription = cx.observe(&nav_search, |_, _, cx| cx.notify());
        let window_attention_add = Self::window_attention_input(locale, window, cx);
        let page = if state.active_operation.is_some() {
            Page::Installing
        } else {
            Page::Home
        };
        let bus_task = bus_owner.as_ref().map(|owner| {
            let requests = owner.requests.clone();
            cx.spawn_in(window, async move |this, cx| {
                while let Ok(request) = requests.recv().await {
                    let shown = this.update_in(cx, |app, window, cx| {
                        app.show_request(request, cx);
                        window.activate_window();
                    });
                    if shown.is_err() {
                        break;
                    }
                }
            })
        });
        let root_focus = cx.focus_handle();
        root_focus.focus(window, cx);
        let pending_plan = state
            .active_operation
            .as_ref()
            .map(|active| active.plan.clone());

        let mut app = Self {
            page,
            history: History::default(),
            wide_collapsed: false,
            narrow_open: false,
            nav_search,
            pending_card: None,
            focused_card: None,
            card_focus: cx.focus_handle(),
            root_focus,
            content_scroll: ScrollHandle::new(),
            device: home::HostRoot::default().device(),
            about_expanded: true,
            dark: true,
            until_dialog_requested: false,
            _bus_owner: bus_owner,
            locale,
            search,
            search_query: String::new(),
            detail_tab: DetailTab::Overview,
            activity_filter: ActivityFilter::All,
            manager,
            state,
            store,
            pending_plan,
            finished_plan: None,
            planning_error,
            execution,
            defaults: DefaultsState::default(),
            catalog_store,
            catalog_status,
            catalog_refreshing: false,
            update_check_requested: false,
            catalog_task: None,
            transfer: None,
            running: None,
            cancel: None,
            adopt_confirm: None,
            unorderable: Vec::new(),
            package_system: None,
            package_system_task: None,
            repair_task: None,
            repair_result: None,
            heal_task: None,
            _subscriptions: vec![subscription],
            _nav_search_subscription: nav_search_subscription,
            _bus_task: bus_task,
            _age_task: cx.spawn(async move |this, cx| {
                loop {
                    smol::Timer::after(AGE_REFRESH).await;
                    let alive = this.update(cx, |app, cx| {
                        if shows_catalog_age(&app.page) {
                            cx.notify();
                        }
                    });
                    if alive.is_err() {
                        break;
                    }
                }
            }),
            update_restart: better_ui::restart::UpdateRestart::new(
                crate::APP_ID,
                "Better Manager",
                "better-manager",
                env!("CARGO_PKG_VERSION"),
                handoff,
                window,
                cx,
            ),
            window_attention: Default::default(),
            window_attention_add,
        };
        if let Some(view) = handoff.and_then(|h| h.state::<crate::restart::ManagerView>()) {
            app.restore_view(view, window, cx);
        }
        app.reconcile_with_host(cx);
        app.refresh_catalog_at_launch(cx);
        app.read_package_system_record(cx);
        app
    }

    /// The Window attention page's "Add an app" field. It is observed so the
    /// suggestions follow what is typed.
    fn window_attention_input(
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(crate::window_attention_page::copy(locale).add_placeholder)
        });
        cx.observe(&input, |_, _, cx| cx.notify()).detach();
        input
    }

    fn nav_search_input(
        locale: Locale,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let value = value.to_string();
        cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(shell(locale).search_placeholder)
                .default_value(value)
        })
    }

    /// What every page's rules read, taken now.
    pub(crate) fn snapshot(&self) -> Snapshot {
        Snapshot {
            locale: self.locale,
            components: self
                .components()
                .into_iter()
                .map(|component| ComponentFacts {
                    id: component.core_id,
                    name: component.name,
                    status: component.state,
                    drifted: component.drift.is_some(),
                })
                .collect(),
            packages_interrupted: self
                .package_system
                .as_ref()
                .is_some_and(|state| state.needs_repair()),
            catalog_warning: CatalogLine::present(
                self.locale,
                &self.catalog_status,
                now_unix_seconds(),
            )
            .warning,
        }
    }

    /// Shows what a command line or a second `better-manager` asked for.
    /// An operation in progress keeps the screen, as `--component` always
    /// did; a component the catalog does not know is ignored.
    pub(crate) fn show_request(&mut self, request: Request, cx: &mut Context<Self>) {
        if self.page == Page::Installing {
            cx.notify();
            return;
        }
        match request {
            Request::Raise => cx.notify(),
            Request::Component(id) => self.open_requested_component(&id, cx),
            Request::Page { page, until_dialog } => {
                self.until_dialog_requested = until_dialog;
                self.navigate(page, cx);
            }
        }
    }

    /// Whether a transaction is running, so Back and Apps lead to it.
    pub(crate) fn transaction_running(&self) -> bool {
        self.state.active_operation.is_some() || self.running.is_some()
    }

    /// Goes back one page, if there is one to go back to.
    pub(crate) fn go_back(&mut self, cx: &mut Context<Self>) {
        let running = self.transaction_running();
        if let Some(page) = crate::navigation::back_target(&mut self.history, &self.page, running) {
            self.enter(page, cx);
        }
    }

    /// Opens the page a search result names and puts the keyboard on its
    /// card.
    pub(crate) fn open_search_result(
        &mut self,
        entry: &SearchEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query = String::new();
        self.reset_nav_search(&query, window, cx);
        self.navigate(entry.route.landing(), cx);
        self.pending_card = Some(entry.card);
        self.focused_card = Some(entry.card);
        cx.notify();
    }

    fn reset_nav_search(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.nav_search = Self::nav_search_input(self.locale, value, window, cx);
        self._nav_search_subscription = cx.observe(&self.nav_search, |_, _, cx| cx.notify());
    }

    /// The search entries the pane's box matches against.
    pub(crate) fn search_index(&self) -> Vec<SearchEntry> {
        crate::pages::search_index(self.locale)
    }

    /// Opens or closes the pane, as ☰ or the compact search button asks.
    pub(crate) fn set_pane_compact(&mut self, compact: bool, narrow: bool, cx: &mut Context<Self>) {
        if narrow {
            self.narrow_open = !compact;
        } else {
            self.wide_collapsed = compact;
        }
        cx.notify();
    }

    /// Reads dpkg's own record, unprivileged and off the UI thread, so Home
    /// can say a package installation was interrupted without starting the
    /// privileged service. The Health screen still asks the service, which
    /// is the one that repairs. A demo window reads nothing from the host.
    fn read_package_system_record(&mut self, cx: &mut Context<Self>) {
        if self.is_demo() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let record = cx
                .background_spawn(async move {
                    manager_core::dpkg_record::read(std::path::Path::new(
                        manager_core::dpkg_record::DPKG_ADMIN_DIR,
                    ))
                })
                .await;
            if let Some(record) = record {
                let _ = this.update(cx, |app, cx| {
                    app.package_system = Some(record);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Asks dpkg what this machine actually has, before anything is drawn.
    ///
    /// Reading the package database is a read-only host query and therefore
    /// allowed outside the privileged boundary. Without it the window counts
    /// only what it installed itself, which on a machine where apt or the
    /// bootstrap installer put Better OS there means every screen says nothing
    /// is installed and no update exists.
    ///
    /// A demo window skips it: its state is a fabrication for screenshots, and
    /// mixing real host facts into it would make it neither.
    fn reconcile_with_host(&mut self, cx: &mut Context<Self>) {
        if self.execution == ExecutionMode::Mock {
            return;
        }
        let mut candidate = self.state.clone();
        let revision = candidate.revision;
        if self
            .manager
            .reconcile(&mut candidate, &DpkgProbe)
            .is_ok_and(|_| candidate.revision != revision)
        {
            self.commit_state(candidate);
        }
        // Read-only, like the reconciliation above, and asked here once rather
        // than on every frame the Health screen draws.
        self.unorderable = self
            .manager
            .unorderable_host_checks(&self.state, &DpkgProbe)
            .unwrap_or_default();
        self.heal_user_units(cx);
    }

    /// Makes the record's "enabled" true of the machine again, off the UI
    /// thread.
    ///
    /// The record already says which components were asked to run. Startup
    /// is where that intent is made true again: the packages install their
    /// units without enabling them, so a machine that installed Better OS
    /// before this pass or upgraded Better Manager afterwards carries a
    /// correct record and a stopped service. Starting a unit waits for
    /// systemd, up to two seconds per unit to see it come up and ten seconds
    /// per `systemctl` call, so it runs on a background thread like the
    /// repair; what it did is written into the state the window has when it
    /// lands. A running transaction owns its units, so nothing starts while
    /// one is in progress.
    fn heal_user_units(&mut self, cx: &mut Context<Self>) {
        if self.is_demo()
            || self.heal_task.is_some()
            || self.running.is_some()
            || self.state.active_operation.is_some()
        {
            return;
        }
        let manager = self.manager.clone();
        let snapshot = self.state.clone();
        self.heal_task = Some(cx.spawn(async move |this, cx| {
            let report = cx
                .background_spawn(async move {
                    let units = SystemdUserUnitController::new();
                    manager
                        .run_user_unit_healing(&snapshot, &DpkgProbe, &units)
                        .unwrap_or_default()
                })
                .await;
            let _ = this.update(cx, |app, cx| app.finish_healing(report, cx));
        }));
    }

    fn finish_healing(
        &mut self,
        report: Vec<manager_core::reconcile::UserUnitHealing>,
        cx: &mut Context<Self>,
    ) {
        self.heal_task = None;
        let mut candidate = self.state.clone();
        if !self
            .manager
            .record_user_unit_healing(&mut candidate, &report)
            .is_empty()
        {
            self.commit_state(candidate);
        }
        cx.notify();
    }

    /// Asks for confirmation before adopting what the machine has, or takes
    /// the confirmation back.
    pub(crate) fn ask_to_adopt_host_state(
        &mut self,
        id: Option<ComponentId>,
        cx: &mut Context<Self>,
    ) {
        self.adopt_confirm = id;
        cx.notify();
    }

    /// Records what dpkg says about one component as the truth.
    ///
    /// The decision is `manager-core`'s: what a record may hold afterwards,
    /// what adoption costs, and what the activity log says happened are the
    /// same whether the request came from here or from the command line. This
    /// window only asks, and saves what comes back.
    pub(crate) fn adopt_host_state(&mut self, id: &ComponentId, cx: &mut Context<Self>) {
        self.adopt_confirm = None;
        // A demo window's state is a fabrication for screenshots; writing a
        // real machine's package state into it would make it neither.
        if self.execution == ExecutionMode::Mock {
            cx.notify();
            return;
        }
        let mut candidate = self.state.clone();
        match self
            .manager
            .adopt_host_state(&mut candidate, id, &DpkgProbe)
        {
            Ok(adoption) if adoption.changed => {
                self.commit_state(candidate);
            }
            Ok(_) => {}
            Err(_) => self.planning_error = Some(AppError::Planning),
        }
        cx.notify();
    }

    /// Starts the launch-time catalog refresh, unless this run was told to stay
    /// offline.
    ///
    /// `BETTER_MANAGER_OFFLINE=1` exists so a headless smoke, a test, or a
    /// machine with no network can open the window without a fetch being
    /// attempted at all. It does not fake a result: the window then shows the
    /// catalog it has and says where that came from.
    fn refresh_catalog_at_launch(&mut self, cx: &mut Context<Self>) {
        if std::env::var_os("BETTER_MANAGER_OFFLINE").is_some() {
            return;
        }
        self.refresh_catalog(cx);
    }

    /// Fetches the published catalog off the UI thread.
    ///
    /// Nothing on screen waits for it. While it runs the button says so, and
    /// when it lands the window adopts whatever it decided — including a
    /// refusal, which arrives as a visible state rather than as silence.
    pub(crate) fn refresh_catalog(&mut self, cx: &mut Context<Self>) {
        if self.catalog_refreshing {
            return;
        }
        self.catalog_refreshing = true;
        let base = self.manager.catalog().clone();
        let status = self.catalog_status.clone();
        let store = self.catalog_store.clone();

        self.catalog_task = Some(cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_spawn(async move {
                    let fetcher = HttpManifestFetcher::from_environment();
                    let outcome = manager_core::catalog::refresh(
                        &fetcher,
                        &base,
                        &status,
                        now_unix_seconds(),
                    );
                    // A cache that cannot be written costs the next start a
                    // fetch and nothing else, so it does not fail the refresh.
                    let _ = cache_refresh(&store, &outcome);
                    outcome
                })
                .await;
            let _ = this.update(cx, |app, cx| app.adopt_catalog(outcome, cx));
        }));
        cx.notify();
    }

    /// Checks for updates, which is the catalog refresh followed by a recount.
    ///
    /// It is deliberately the same code path as the Components screen's button:
    /// two ways to ask the same question that could disagree about the answer
    /// is the defect this avoids. The count itself is derived from the adopted
    /// catalog, so nothing has to be recomputed when the fetch lands.
    pub(crate) fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        self.update_check_requested = true;
        self.refresh_catalog(cx);
        cx.notify();
    }

    /// What the manual check has to say right now.
    pub(crate) fn update_check(&self) -> UpdateCheck {
        UpdateCheck::present(
            self.locale,
            self.update_check_requested,
            self.catalog_refreshing,
            &self.catalog_status,
            self.update_plan_count(),
        )
    }

    /// Adopts what a refresh decided: the catalog it settled on and the state
    /// that describes it. The host profile is kept, because a catalog refresh
    /// says nothing about the machine.
    fn adopt_catalog(&mut self, outcome: RefreshOutcome, cx: &mut Context<Self>) {
        self.catalog_refreshing = false;
        self.manager = Manager::new(outcome.catalog, self.manager.profile().clone());
        self.catalog_status = outcome.status;
        cx.notify();
    }

    /// Whether this window can actually change the machine.
    pub(crate) fn is_demo(&self) -> bool {
        self.execution == ExecutionMode::Mock
    }

    /// Whether the user may still abandon the running transaction.
    ///
    /// A simulation can always be abandoned. A real one only until it has been
    /// handed to the privileged service: after that the host may already have
    /// changed, and offering a cancel would promise a restoration nothing
    /// performed.
    pub(crate) fn can_cancel_now(&self) -> bool {
        if self.is_demo() {
            return true;
        }
        self.state
            .active_operation
            .as_ref()
            .map(|active| active.stage == OperationStage::Downloading)
            .unwrap_or(true)
    }

    fn subscribe_to_search(
        search: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        let search_for_callback = search.clone();
        cx.subscribe_in(search, window, move |this, _, event: &InputEvent, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.search_query = search_for_callback.read(cx).value().to_string();
                cx.notify();
            }
        })
    }

    fn commit_state(&mut self, candidate: ManagerState) -> bool {
        match self.store.save(&candidate) {
            Ok(()) => {
                self.state = candidate;
                if self.planning_error == Some(AppError::Storage) {
                    self.planning_error = None;
                }
                true
            }
            Err(_) => {
                self.planning_error = Some(AppError::Storage);
                false
            }
        }
    }

    pub(crate) fn set_locale(
        &mut self,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut candidate = self.state.clone();
        let mut settings = candidate.settings.clone();
        settings.locale = stored_from_locale(locale);
        candidate.update_settings(settings);
        if !self.commit_state(candidate) {
            cx.notify();
            return;
        }

        self.locale = locale;
        // The pane's search box carries its placeholder in the language it
        // was made in, so it is made again, keeping what was typed.
        let typed = self.nav_search.read(cx).value().to_string();
        self.reset_nav_search(&typed, window, cx);
        self.window_attention_add = Self::window_attention_input(locale, window, cx);
        let current_value = self.search.read(cx).value().to_string();
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(copy(locale).search)
                .default_value(current_value)
        });
        let subscription = Self::subscribe_to_search(&search, window, cx);
        self.search = search;
        self._subscriptions.clear();
        self._subscriptions.push(subscription);
        cx.notify();
    }

    pub(crate) fn set_theme(
        &mut self,
        theme: StoredTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut candidate = self.state.clone();
        let mut settings = candidate.settings.clone();
        settings.theme = theme;
        candidate.update_settings(settings);
        if !self.commit_state(candidate) {
            cx.notify();
            return;
        }
        appearance_of(theme).apply_and_follow(window, cx);
        cx.notify();
    }

    pub(crate) fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(copy(self.locale).search));
        let subscription = Self::subscribe_to_search(&search, window, cx);
        self.search = search;
        self.search_query.clear();
        self._subscriptions.clear();
        self._subscriptions.push(subscription);
        cx.notify();
    }

    /// Goes to a page, remembering the one it leaves for the back button.
    pub(crate) fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        self.history.visit(&self.page, &page);
        self.enter(page, cx);
    }

    /// Shows a page without touching the history.
    fn enter(&mut self, page: Page, cx: &mut Context<Self>) {
        // The Health screen is where an interrupted dpkg run is shown, so it
        // asks the privileged service each time it is opened rather than at
        // launch: opening the window should not start a root process.
        if matches!(page, Page::Health | Page::DoctorResults) {
            self.check_package_system(cx);
        }
        if page == Page::Home {
            // The battery moves, and dpkg's record may have; both are read
            // again each time Home is shown rather than polled.
            self.device = home::HostRoot::default().device();
            self.read_package_system_record(cx);
        }
        // Default apps reads what every integration says the first time it is
        // shown, as its sidebar item used to.
        if page == Page::Defaults && self.defaults.report.is_none() && !self.defaults.busy {
            self.refresh_defaults(cx);
        }
        // Window attention reads the shell and the service each time it is
        // shown: either can change behind the window's back.
        if page == Page::WindowAttention {
            self.refresh_window_attention(cx);
        }
        if page != self.page {
            self.focused_card = None;
            self.pending_card = None;
            self.content_scroll.set_offset(gpui::Point::default());
        }
        // A narrow window's open pane closes once a page is chosen, as
        // WinUI's overlay pane does.
        self.narrow_open = false;
        self.page = page;
        cx.notify();
    }

    /// Asks the privileged service what dpkg's own record says, off the UI
    /// thread. A demo window never asks. A service that cannot be reached
    /// leaves the last answer in place rather than inventing a clean one.
    pub(crate) fn check_package_system(&mut self, cx: &mut Context<Self>) {
        if self.is_demo() || self.package_system_task.is_some() {
            return;
        }
        self.package_system_task = Some(cx.spawn(async move |this, cx| {
            let answer = cx
                .background_spawn(async move {
                    let service = DbusPrivilegedExecutor::connect().ok()?;
                    repair::package_system_state(&service).ok()
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.package_system_task = None;
                if answer.is_some() {
                    app.package_system = answer;
                }
                cx.notify();
            });
        }));
    }

    /// Asks the privileged service to finish an interrupted dpkg run.
    ///
    /// `manager-core` asks and words the result; the service decides whether a
    /// repair is needed and runs it. The request runs off the UI thread because
    /// polkit's password prompt holds it for as long as the person takes. The
    /// result is recorded against the state the window has when it lands, not
    /// the state it had when the button was pressed.
    pub(crate) fn repair_package_system(&mut self, cx: &mut Context<Self>) {
        if self.is_demo() || self.repair_task.is_some() || self.running.is_some() {
            return;
        }
        self.repair_result = None;
        self.repair_task = Some(cx.spawn(async move |this, cx| {
            let report = cx
                .background_spawn(async move {
                    match DbusPrivilegedExecutor::connect() {
                        Ok(service) => repair::request_repair(&service),
                        Err(error) => repair::unreachable(&error),
                    }
                })
                .await;
            let _ = this.update(cx, |app, cx| app.finish_repair(report, cx));
        }));
        cx.notify();
    }

    fn finish_repair(&mut self, report: repair::RepairReport, cx: &mut Context<Self>) {
        self.repair_task = None;
        let mut candidate = self.state.clone();
        repair::record_repair(&mut candidate, &report);
        self.commit_state(candidate);
        if let Some(remaining) = report.remaining {
            self.package_system = Some(remaining);
        }
        self.repair_result = Some(report.result);
        // Finishing dpkg's work can move a component's installed version, and
        // dpkg is where the manager reads that.
        self.reconcile_with_host(cx);
        cx.notify();
    }

    /// Closes Home's first-run info bar for good.
    pub(crate) fn complete_onboarding(&mut self, cx: &mut Context<Self>) {
        let mut candidate = self.state.clone();
        let mut settings = candidate.settings.clone();
        settings.onboarding_complete = true;
        candidate.update_settings(settings);
        self.commit_state(candidate);
        cx.notify();
    }

    pub(crate) fn prepare_update_all(&mut self, cx: &mut Context<Self>) {
        match self.manager.plan_all_in_mode(&self.state, self.execution) {
            Ok(plan) if !plan.is_empty() => {
                self.pending_plan = Some(plan);
                self.planning_error = None;
                self.navigate(Page::ReviewChanges, cx);
            }
            Ok(_) => {
                self.pending_plan = None;
                self.navigate(Page::Updates, cx);
            }
            Err(_) => self.show_planning_error(Page::Updates, cx),
        }
    }

    pub(crate) fn prepare_component_change(&mut self, id: &ComponentId, cx: &mut Context<Self>) {
        let Some(component) = self.component_by_id(id) else {
            self.show_planning_error(Page::Components, cx);
            return;
        };
        let operation = match component.state {
            ComponentStatus::Available => Some(DesiredOperation::Install),
            ComponentStatus::UpdateAvailable => Some(DesiredOperation::Update),
            ComponentStatus::Disabled => Some(DesiredOperation::Enable),
            ComponentStatus::RestoreAvailable => Some(DesiredOperation::Restore),
            ComponentStatus::Degraded | ComponentStatus::Failed => Some(self.retry_operation(id)),
            _ => None,
        };
        if let Some(operation) = operation {
            self.prepare_component_operation(id, operation, cx);
        } else {
            self.open_component(id, cx);
        }
    }

    /// What "try again" means for a component that recorded a failure.
    ///
    /// An install that failed before anything was applied leaves nothing
    /// installed, and a health check on nothing installed is refused by the
    /// planner — which is how a failed component ended up with a red tag and no
    /// working action at all. Retrying the install is the operation that
    /// actually applies there.
    pub(crate) fn retry_operation(&self, id: &ComponentId) -> DesiredOperation {
        let installed = self
            .state
            .component(id)
            .and_then(|record| record.installed_version.as_ref())
            .is_some();
        if installed {
            DesiredOperation::Verify
        } else {
            DesiredOperation::Install
        }
    }

    /// Whether a transaction would replace this application's own package.
    pub(crate) fn updates_the_manager(steps: &[PlanStep]) -> bool {
        steps.iter().any(|step| {
            manager_core::is_self_component(&step.component) && step.operation.mutates_component()
        })
    }

    pub(crate) fn prepare_component_operation(
        &mut self,
        id: &ComponentId,
        operation: DesiredOperation,
        cx: &mut Context<Self>,
    ) {
        match self.plan_operation(id, operation) {
            Ok(plan) => {
                self.pending_plan = Some(plan);
                self.planning_error = None;
                self.navigate(Page::ReviewChanges, cx);
            }
            Err(_) => self.show_planning_error(Page::Components, cx),
        }
    }

    /// The one planning path for an operation on one component.
    ///
    /// A real window asks dpkg first, so a package it holds at a version
    /// nothing can order is refused before a plan exists (ticket 69). A demo
    /// plans from its fabricated state and never reads the host.
    fn plan_operation(
        &self,
        id: &ComponentId,
        operation: DesiredOperation,
    ) -> Result<TransactionPlan, manager_core::ManagerError> {
        match self.execution {
            ExecutionMode::Mock => {
                self.manager
                    .plan_in_mode(&self.state, id, operation, ExecutionMode::Mock)
            }
            ExecutionMode::Real => self.manager.plan_on_host(
                &self.state,
                id,
                operation,
                ExecutionMode::Real,
                &DpkgProbe,
            ),
        }
    }

    /// The doctor finding for a component dpkg holds at a version nothing can
    /// order, from the last reconciliation.
    pub(crate) fn unorderable_check(&self, id: &ComponentId) -> Option<&DoctorCheck> {
        self.unorderable
            .iter()
            .find(|check| check.component.as_ref() == Some(id))
    }

    pub(crate) fn begin_install(&mut self, cx: &mut Context<Self>) {
        let Some(plan) = self.pending_plan.clone() else {
            self.show_planning_error(Page::Updates, cx);
            return;
        };
        let mut candidate = self.state.clone();
        match self.manager.begin(&mut candidate, plan) {
            Ok(_) => {
                if self.commit_state(candidate) {
                    self.navigate(Page::Installing, cx);
                    if !self.is_demo() {
                        self.run_real_transaction(cx);
                    }
                } else {
                    cx.notify();
                }
            }
            Err(_) => self.show_planning_error(Page::ReviewChanges, cx),
        }
    }

    /// Runs the whole transaction off the UI thread.
    ///
    /// The runner owns the state and every save for the duration; the window
    /// adopts what it reports rather than keeping a second copy in step. All
    /// network and IPC happens on the background thread, so the interface keeps
    /// drawing while packages are being fetched and applied.
    fn run_real_transaction(&mut self, cx: &mut Context<Self>) {
        let manager = self.manager.clone();
        let store = self.store.clone();
        let mut state = self.state.clone();
        let profile = self.manager.profile().clone();
        let cancel = CancelToken::new();
        self.cancel = Some(cancel.clone());
        self.transfer = None;

        let (sender, receiver) =
            smol::channel::unbounded::<crate::runner_end::WorkerEvent<RunnerEvent>>();
        let worker = cx.background_spawn(async move {
            // However this ends — no service, a result, an error part way, or
            // a panic — the window is told once, after everything else.
            let ended = sender.clone();
            let _ends = crate::runner_end::EndsOnce::new(move || {
                let _ = ended.send_blocking(crate::runner_end::WorkerEvent::Ended);
            });
            let downloader = HttpDownloader::new(ArtifactCache::from_default_path());
            let executor = match DbusPrivilegedExecutor::connect() {
                Ok(executor) => executor,
                Err(_) => {
                    // Nothing was applied, so abandoning is honest here.
                    let _ = manager.cancel(&mut state);
                    let _ = store.save(&state);
                    let _ = sender
                        .send(crate::runner_end::WorkerEvent::Runner(
                            RunnerEvent::StateSaved(Box::new(state)),
                        ))
                        .await;
                    return;
                }
            };
            let user_units = SystemdUserUnitController::new();
            let driver = RealDriver::new(
                &downloader,
                &executor,
                &manager_platform::dpkg::DpkgProbe,
                &better_core::package_health::SystemHealthProbe,
                uuid::Uuid::new_v4().to_string(),
                profile,
            )
            .with_user_units(&user_units);
            let mut runner = TransactionRunner::new(&manager, Box::new(driver), &store)
                .with_cancel_token(cancel);
            let _ = runner.run(&mut state, &mut |event| {
                let _ = sender.send_blocking(crate::runner_end::WorkerEvent::Runner(event));
            });
        });

        let pump = cx.spawn(async move |this, cx| {
            while let Ok(event) = receiver.recv().await {
                let applied = this.update(cx, |app, cx| match event {
                    crate::runner_end::WorkerEvent::Runner(event) => {
                        app.apply_runner_event(event, cx)
                    }
                    crate::runner_end::WorkerEvent::Ended => app.runner_ended(cx),
                });
                if applied.is_err() {
                    break;
                }
            }
        });
        // Keeping the worker alive is the pump's job; the window only has to
        // hold one handle.
        self.running = Some(cx.background_spawn(async move {
            worker.await;
            pump.await;
        }));
    }

    /// The background work is over, however it ended. Its task was kept as
    /// `running` and never cleared, so `transaction_running()` stayed true
    /// after the first transaction: the repair stayed disabled, the startup
    /// unit pass was skipped, Back led to a finished progress screen, and a
    /// restart after an update would wait forever (ticket 105). The task is
    /// let finish rather than cancelled, and an end with no finish reported
    /// leaves the installation screen.
    fn runner_ended(&mut self, cx: &mut Context<Self>) {
        if let Some(running) = self.running.take() {
            running.detach();
        }
        self.transfer = None;
        self.cancel = None;
        if let Some(page) = crate::runner_end::page_after_unfinished_end(
            self.page == Page::Installing,
            self.state.active_operation.is_some(),
        ) {
            self.navigate(page, cx);
        }
        cx.notify();
    }

    /// Adopts what the background transaction reported.
    fn apply_runner_event(&mut self, event: RunnerEvent, cx: &mut Context<Self>) {
        match event {
            RunnerEvent::Progress(StageProgress::Downloading {
                component,
                received_bytes,
                total_bytes,
            }) => {
                self.transfer = Some(Transfer {
                    component,
                    received_bytes,
                    total_bytes,
                });
                cx.notify();
            }
            RunnerEvent::Progress(StageProgress::Applying { .. })
            | RunnerEvent::StageEntered(_) => {
                self.transfer = None;
                cx.notify();
            }
            RunnerEvent::StateSaved(state) => {
                self.state = *state;
                match self.state.active_operation.as_ref() {
                    Some(active) => self.pending_plan = Some(active.plan.clone()),
                    // The transaction is over. The plan stops being something
                    // awaiting review — leaving it there made every component
                    // in it keep offering "Review changes" — and becomes what
                    // the result screens describe.
                    None => self.finished_plan = self.pending_plan.take(),
                }
                cx.notify();
            }
            RunnerEvent::Finished(progress) => {
                self.transfer = None;
                self.cancel = None;
                match progress {
                    OperationProgress::Finished { operation } => self.navigate(
                        if operation == DesiredOperation::Restore {
                            Page::Restored
                        } else {
                            Page::Finished
                        },
                        cx,
                    ),
                    OperationProgress::Failed { .. } => {
                        // A failure is the moment a person needs to know if
                        // dpkg is the reason, before they try again.
                        self.check_package_system(cx);
                        self.navigate(Page::Restore, cx);
                    }
                    OperationProgress::InProgress { .. } => cx.notify(),
                }
            }
        }
    }

    pub(crate) fn advance_install(&mut self, cx: &mut Context<Self>) {
        let mut candidate = self.state.clone();
        match self
            .manager
            .advance_mock(&mut candidate, MockOutcome::Succeed)
        {
            Ok(progress) => {
                if !self.commit_state(candidate) {
                    cx.notify();
                    return;
                }
                if !matches!(progress, OperationProgress::InProgress { .. }) {
                    self.finished_plan = self.pending_plan.take();
                }
                match progress {
                    OperationProgress::InProgress { .. } => self.navigate(Page::Installing, cx),
                    OperationProgress::Finished { operation } => self.navigate(
                        if operation == DesiredOperation::Restore {
                            Page::Restored
                        } else {
                            Page::Finished
                        },
                        cx,
                    ),
                    OperationProgress::Failed { .. } => self.navigate(Page::Restore, cx),
                }
            }
            Err(_) => self.show_planning_error(Page::Installing, cx),
        }
    }

    pub(crate) fn cancel_install(&mut self, cx: &mut Context<Self>) {
        // A real transaction is cancelled by asking the running work to stop,
        // which it only honors while stopping still leaves the host as it was.
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
            cx.notify();
            return;
        }
        let mut candidate = self.state.clone();
        match self.manager.cancel(&mut candidate) {
            Ok(()) => {
                if self.commit_state(candidate) {
                    self.pending_plan = None;
                    self.navigate(Page::Updates, cx);
                } else {
                    cx.notify();
                }
            }
            Err(_) => self.show_planning_error(Page::Installing, cx),
        }
    }

    pub(crate) fn prepare_recovery(
        &mut self,
        component: ComponentId,
        operation: DesiredOperation,
        cx: &mut Context<Self>,
    ) {
        match self.plan_operation(&component, operation) {
            Ok(plan) => {
                self.pending_plan = Some(plan);
                self.planning_error = None;
                self.navigate(Page::ReviewChanges, cx);
            }
            Err(_) => self.show_planning_error(Page::Restore, cx),
        }
    }

    pub(crate) fn set_component_filter(
        &mut self,
        filter: ComponentFilterPreference,
        cx: &mut Context<Self>,
    ) {
        let mut candidate = self.state.clone();
        let mut settings = candidate.settings.clone();
        settings.component_filter = filter;
        candidate.update_settings(settings);
        self.commit_state(candidate);
        cx.notify();
    }

    pub(crate) fn toggle_check_updates(&mut self, cx: &mut Context<Self>) {
        self.update_settings(|settings| settings.check_updates = !settings.check_updates);
        cx.notify();
    }

    fn update_settings(&mut self, update: impl FnOnce(&mut ManagerSettings)) {
        let mut candidate = self.state.clone();
        let mut settings = candidate.settings.clone();
        update(&mut settings);
        candidate.update_settings(settings);
        self.commit_state(candidate);
    }

    pub(crate) fn clear_activity(&mut self, cx: &mut Context<Self>) {
        let mut candidate = self.state.clone();
        candidate.clear_activity();
        self.commit_state(candidate);
        cx.notify();
    }

    pub(crate) fn components(&self) -> Vec<ComponentInfo> {
        self.manager
            .manifests()
            .filter_map(|manifest| {
                let record = self.state.component(&manifest.id);
                let state = self.manager.status(&self.state, &manifest.id).ok()?;
                Some(ComponentInfo::present(
                    manifest,
                    record,
                    state,
                    translated_component(self.locale, &manifest.id),
                ))
            })
            .collect()
    }

    pub(crate) fn component_by_id(&self, id: &ComponentId) -> Option<ComponentInfo> {
        self.components()
            .into_iter()
            .find(|component| &component.core_id == id)
    }

    pub(crate) fn plan_component_name(&self, id: &ComponentId) -> String {
        translated_component(self.locale, id)
            .map(|translation| translation.name.to_string())
            .or_else(|| {
                self.manager
                    .manifests()
                    .find(|manifest| &manifest.id == id)
                    .map(|manifest| manifest.display_name.clone())
            })
            .unwrap_or_else(|| id.to_string())
    }

    pub(crate) fn update_plan_count(&self) -> usize {
        self.components()
            .into_iter()
            .filter(|component| component.state == ComponentStatus::UpdateAvailable)
            .count()
    }

    pub(crate) fn is_pending(&self, id: &ComponentId) -> bool {
        self.pending_plan
            .as_ref()
            .is_some_and(|plan| plan.steps().iter().any(|step| &step.component == id))
    }

    pub(crate) fn pending_steps(&self) -> Vec<PlanStep> {
        self.pending_plan
            .as_ref()
            .map(|plan| plan.steps().to_vec())
            .unwrap_or_default()
    }

    /// What the pending transaction as a whole leaves to happen afterwards,
    /// for the review's summary row; `None` hides the row.
    pub(crate) fn pending_restart_label(&self) -> Option<&'static str> {
        let plan = self.pending_plan.as_ref()?;
        let requirement = plan.restart_requirement();
        let operations: Vec<DesiredOperation> = plan
            .steps()
            .iter()
            .filter(|step| step.restart_requirement == requirement)
            .map(|step| step.operation)
            .collect();
        crate::components::restart_effect(self.locale, plan_operation(&operations), requirement)
    }

    pub(crate) fn pending_disk_space(&self) -> DiskSpaceCheck {
        self.pending_plan
            .as_ref()
            .map(|plan| plan.disk_space())
            .unwrap_or(DiskSpaceCheck::NotRequired)
    }

    /// The steps of the transaction that just ran.
    pub(crate) fn finished_steps(&self) -> Vec<PlanStep> {
        self.finished_plan
            .as_ref()
            .map(|plan| plan.steps().to_vec())
            .unwrap_or_default()
    }

    pub(crate) fn active_steps(&self) -> Vec<PlanStep> {
        self.state
            .active_operation
            .as_ref()
            .map(|active| active.plan.steps().to_vec())
            .unwrap_or_else(|| self.pending_steps())
    }

    /// Opens a component's page because another window asked for it
    /// (`better-manager --component <id>`). An operation in progress and the
    /// first-run page keep the screen; an unknown component is ignored.
    pub(crate) fn open_requested_component(&mut self, id: &ComponentId, cx: &mut Context<Self>) {
        if opens_requested_component(&self.page, self.component_by_id(id).is_some()) {
            self.open_component(id, cx);
        }
    }

    pub(crate) fn open_component(&mut self, id: &ComponentId, cx: &mut Context<Self>) {
        self.detail_tab = DetailTab::Overview;
        self.navigate(Page::ComponentDetail(id.clone()), cx);
    }

    pub(crate) fn selected_component(&self) -> Option<ComponentInfo> {
        match &self.page {
            Page::ComponentDetail(id) => self.component_by_id(id),
            _ => None,
        }
    }

    /// Presentation text for a value a component did not declare.
    pub(crate) fn declared_or_not(&self, value: &str) -> String {
        if value.trim().is_empty() {
            copy(self.locale).not_declared.to_string()
        } else {
            value.to_string()
        }
    }

    pub(crate) fn component_name_for_core(&self, id: Option<&ComponentId>) -> String {
        id.map(|id| self.plan_component_name(id))
            .unwrap_or_else(|| copy(self.locale).manager.to_string())
    }

    pub(crate) fn stage(&self) -> Option<OperationStage> {
        self.state
            .active_operation
            .as_ref()
            .map(|active| active.stage)
    }

    pub(crate) fn current_failure(&self) -> Option<(ComponentId, manager_core::FailureRecord)> {
        self.state
            .components
            .iter()
            .find_map(|(id, record)| record.failure.clone().map(|failure| (id.clone(), failure)))
    }

    pub(crate) fn doctor_checks(&self) -> Vec<DoctorCheck> {
        let mut checks = self.manager.doctor(&self.state).unwrap_or_default();
        checks.extend(self.unorderable.iter().cloned());
        checks
    }

    pub(crate) fn activity_matches(&self, entry: &ActivityRecord) -> bool {
        match self.activity_filter {
            ActivityFilter::All => true,
            ActivityFilter::Success => matches!(
                entry.kind,
                ActivityKind::Success | ActivityKind::RecoverySuccess
            ),
            ActivityFilter::Warning => matches!(
                entry.kind,
                ActivityKind::Warning
                    | ActivityKind::RecoveryPartial
                    | ActivityKind::ManualRecovery
            ),
            ActivityFilter::Failure => entry.kind == ActivityKind::Failure,
        }
    }

    pub(crate) fn stage_label(&self, stage: OperationStage) -> &'static str {
        let c = copy(self.locale);
        match stage {
            OperationStage::Downloading => c.downloading,
            OperationStage::Installing => c.installing_files,
            OperationStage::ApplyingSettings => c.applying_settings,
            OperationStage::CheckingHealth => c.checking_works,
        }
    }

    pub(crate) fn evidence_label(
        &self,
        evidence: Option<&str>,
        detail: Option<&str>,
    ) -> &'static str {
        let c = copy(self.locale);
        match evidence {
            Some("mock_failure_at_downloading") => c.downloading,
            Some("mock_failure_at_installing") => c.installing_files,
            Some("mock_failure_at_applying_settings") => c.applying_settings,
            Some("mock_failure_at_checking_health") => c.checking_works,
            Some("mock_restore_recheck_failed") => c.restore_recheck_failed,
            Some("mock_restore_rechecked")
            | Some("mock_health_check_passed")
            | Some("health_check_passed") => c.passed,
            Some("mock_operation_finished") => c.finished,
            Some("mock_operation_cancelled") | Some("operation.cancelled") => c.cancel,
            // Real execution reports what actually went wrong.
            Some("download.network") => c.evidence_download_network,
            Some("download.checksum_mismatch") => c.evidence_checksum_mismatch,
            Some("daemon.unavailable") | Some("daemon.not_approved") => {
                c.evidence_daemon_unavailable
            }
            Some("daemon.interrupted") => c.evidence_daemon_interrupted,
            Some("daemon.polkit_denied") => c.evidence_polkit_denied,
            Some("restore.artifact_missing") => c.evidence_restore_artifact_missing,
            Some(other) if other.starts_with("daemon.error.apt_busy") => c.evidence_apt_busy,
            Some(other) if other.starts_with("daemon.error.apt_failed") => c.evidence_apt_failed,
            Some(other)
                if other.starts_with("daemon.error.health_failed") || other == "health.failed" =>
            {
                c.evidence_health_failed
            }
            Some("health.undetermined") => c.evidence_health_undetermined,
            Some(other) if other.starts_with("daemon.error.state_drift") => c.evidence_state_drift,
            Some(other) if other.starts_with("daemon.error.dpkg_interrupted") => {
                c.evidence_dpkg_interrupted
            }
            Some(other) if other.starts_with("daemon.error.repair_failed") => {
                c.evidence_repair_failed
            }
            Some("package_system.repaired") => c.evidence_package_system_repaired,
            Some("package_system.still_interrupted") => c.evidence_package_system_still_interrupted,
            Some("package_system.nothing_to_repair") => c.repair_result_nothing,
            Some(other) if other.starts_with("user_units.failed") => c.evidence_user_units_failed,
            Some(other) if other.starts_with("user_units.not_running") => {
                c.evidence_user_units_not_running
            }
            Some(other) if other.starts_with("user_units.started") => c.evidence_user_units_started,
            Some(other) if other.starts_with("user_units.masked") => c.evidence_user_units_masked,
            Some(other) if other.starts_with("user_units.no_session") => {
                c.evidence_user_units_no_session
            }
            Some(other) if other.starts_with("host.externally_upgraded") => {
                c.evidence_externally_upgraded
            }
            Some(other) if other.starts_with("host.state_adopted") => c.evidence_state_adopted,
            Some(other) if plan_rejection_is_host_mismatch(other, detail) => {
                c.evidence_plan_rejected
            }
            Some(other) if other.starts_with("daemon.") => c.evidence_daemon_refused,
            _ => c.none,
        }
    }

    pub(crate) fn operation_label(&self, operation: DesiredOperation) -> &'static str {
        let c = copy(self.locale);
        match operation {
            DesiredOperation::Install => c.install,
            DesiredOperation::Update => c.update,
            DesiredOperation::Enable => c.enable,
            DesiredOperation::Disable => c.disable,
            DesiredOperation::Verify => c.health_check_label,
            DesiredOperation::Restore => c.restore_previous,
            DesiredOperation::Remove => c.remove,
        }
    }

    pub(crate) fn error_message(&self) -> &'static str {
        let c = copy(self.locale);
        match self.planning_error {
            Some(AppError::Planning) => c.planning_failed_detail,
            Some(AppError::Storage) => c.storage_error,
            Some(AppError::UnsupportedHost) => c.unsupported_host,
            None => c.none,
        }
    }

    fn show_planning_error(&mut self, page: Page, cx: &mut Context<Self>) {
        self.planning_error = Some(AppError::Planning);
        self.navigate(page, cx);
    }
}

/// The kit's appearance for a stored choice. The choice is stored once, in
/// the manager's settings, for every page (spec section 10).
/// How often a page that says how long ago the version list was checked
/// redraws: the coarsest unit that line uses below an hour is a minute.
pub(crate) const AGE_REFRESH: std::time::Duration = std::time::Duration::from_secs(60);

/// The pages that show the version list's age: Updates, and Apps' component
/// list.
pub(crate) fn shows_catalog_age(page: &Page) -> bool {
    matches!(page, Page::Updates | Page::Components)
}

/// The operation whose wording a plan's summary row uses, from the
/// operations of the steps that set its restart requirement: an update only
/// when every one of them is an update, nothing to say only when every one
/// takes the component away, and the neutral wording otherwise.
pub(crate) fn plan_operation(operations: &[DesiredOperation]) -> DesiredOperation {
    if !operations.is_empty() && operations.iter().all(|op| *op == DesiredOperation::Update) {
        DesiredOperation::Update
    } else if !operations.is_empty()
        && operations
            .iter()
            .all(|op| matches!(op, DesiredOperation::Remove | DesiredOperation::Disable))
    {
        DesiredOperation::Remove
    } else {
        DesiredOperation::Install
    }
}

pub(crate) fn appearance_of(theme: StoredTheme) -> Appearance {
    match theme {
        StoredTheme::Dark => Appearance::Dark,
        StoredTheme::Light => Appearance::Light,
        StoredTheme::System => Appearance::System,
    }
}

/// The copy this build ships for a first-party component.
#[derive(Clone, Copy)]
pub(crate) struct ComponentTranslation {
    pub(crate) name: &'static str,
    pub(crate) summary: &'static str,
    pub(crate) detail: &'static str,
}

/// The copy this build ships for a first-party component. A component without
/// a translation is presented from its own manifest instead of being dropped
/// from the catalog.
pub(crate) fn translated_component(
    locale: Locale,
    id: &ComponentId,
) -> Option<ComponentTranslation> {
    let c = copy(locale);
    match id.as_str() {
        "better-manager" => Some(ComponentTranslation {
            name: c.manager_name,
            summary: c.manager_purpose,
            detail: c.manager_detail,
        }),
        "better-monitor" => Some(ComponentTranslation {
            name: c.monitor_name,
            summary: c.monitor_purpose,
            detail: c.monitor_detail,
        }),
        "better-files" => Some(ComponentTranslation {
            name: c.files_name,
            summary: c.files_purpose,
            detail: c.files_detail,
        }),
        _ => None,
    }
}

/// The compiled-in catalog, as a manager. The window itself starts from the
/// cache when there is one; this is what the tests plan against, so a test is
/// never at the mercy of what a previous run cached.
#[cfg(test)]
fn catalog_manager() -> Manager {
    Manager::probe(
        manager_core::catalog::built_in_catalog(),
        &manager_platform::MockPlatform::default(),
    )
    .expect("the mock platform always reports a profile")
}

#[cfg(test)]
pub(crate) fn demo_manager() -> (Manager, ManagerState) {
    let manager = catalog_manager();
    let mut state = ManagerState::default();
    // Installed at the version the catalog offers, so this component is the
    // "already current" half of the demo state. Reading it from the manifest
    // keeps that true across a release version bump.
    let manager_id = ComponentId::new("better-manager").expect("example id must be valid");
    let manager_version = manager
        .manifests()
        .find(|manifest| manifest.id == manager_id)
        .expect("the built-in catalog declares better-manager")
        .version
        .to_string();
    state.set_installed(manager_id, manager_version, true);
    state.set_installed(
        ComponentId::new("better-monitor").expect("example id must be valid"),
        "0.0.1",
        true,
    );
    (manager, state)
}

fn locale_from_stored(locale: StoredLocale) -> Locale {
    match locale {
        StoredLocale::System => Locale::System,
        StoredLocale::EnUs => Locale::EnUs,
        StoredLocale::ZhTw => Locale::ZhTw,
    }
}

fn stored_from_locale(locale: Locale) -> StoredLocale {
    match locale {
        Locale::System => StoredLocale::System,
        Locale::EnUs => StoredLocale::EnUs,
        Locale::ZhTw => StoredLocale::ZhTw,
    }
}

/// Whether a refusal key says the plan was built for a different machine.
///
/// Only the daemon's release and architecture checks mean that. Any other
/// refusal — including one the client raised before sending anything — is a
/// refusal for its own stated reason, and calling it a machine mismatch sends
/// the person looking for a problem the machine does not have.
pub(crate) fn plan_rejection_is_host_mismatch(evidence: &str, detail: Option<&str>) -> bool {
    // A failure record keeps the service's words apart from the key; an older
    // record and an activity entry carry them after a colon in the key itself.
    let (key, detail) = match detail {
        Some(detail) => (evidence, detail),
        None => match evidence.split_once(':') {
            Some(parts) => parts,
            None => return false,
        },
    };
    key == "daemon.error.plan_rejected"
        && (detail.starts_with("plan targets release ")
            || detail.starts_with("plan targets architecture "))
}

/// The sentence an activity row shows for an entry with no stage.
///
/// The row has no room for the failure's detail, and the startup pass's
/// entries carry the unit in the evidence itself (`user_units.failed:<unit>`),
/// so the unit is named after the sentence. Anything else reads as its label.
pub(crate) fn activity_evidence_text(label: &str, evidence: Option<&str>) -> String {
    match evidence
        .and_then(|evidence| evidence.strip_prefix("user_units."))
        .and_then(|rest| rest.split_once(':'))
    {
        Some((_, units)) if !units.trim().is_empty() => {
            format!(
                "{label}: {}",
                units.split(',').collect::<Vec<_>>().join(", ")
            )
        }
        _ => label.to_string(),
    }
}

/// Whether a requested component's page may replace the page the window
/// is on: not over an operation in progress, and only for a component the
/// catalog knows.
pub(crate) fn opens_requested_component(page: &Page, known: bool) -> bool {
    known && !matches!(page, Page::Installing)
}

impl ManagerApp {
    /// Puts back the view a restart after an update handed over. The page the
    /// state file put the window on — an operation still in progress — wins.
    fn restore_view(
        &mut self,
        view: crate::restart::ManagerView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_plan.is_none() {
            self.pending_plan = view.pending_plan;
        }
        self.finished_plan = view.finished_plan;
        let pending = self.pending_plan.is_some();
        let finished = self.finished_plan.is_some();
        let knows = |id: &ComponentId| self.component_by_id(id).is_some();
        let page = view.page.page(&knows, pending, finished);
        let back = view
            .back
            .iter()
            .map(|key| key.page(&knows, pending, finished))
            .collect();
        self.history = crate::navigation::History::from_pages(back);
        self.detail_tab = view.detail_tab.tab();
        self.activity_filter = view.activity_filter.filter();
        self.wide_collapsed = view.wide_collapsed;
        self.about_expanded = view.about_expanded;
        self.update_check_requested = view.update_check_requested;
        if !view.search.is_empty() {
            let text = view.search.clone();
            self.search
                .update(cx, |state, cx| state.set_value(text, window, cx));
            self.search_query = view.search;
        }
        if !view.nav_search.is_empty() {
            let text = view.nav_search;
            self.nav_search
                .update(cx, |state, cx| state.set_value(text, window, cx));
        }
        if self.page != Page::Installing {
            self.enter(page, cx);
            self.narrow_open = view.narrow_open;
            self.content_scroll
                .set_offset(gpui::point(gpui::px(0.0), gpui::px(-view.scroll.max(0.0))));
        }
    }
}

impl better_ui::restart::Restartable for ManagerApp {
    type State = crate::restart::ManagerView;

    fn update_restart(&mut self) -> &mut better_ui::restart::UpdateRestart {
        &mut self.update_restart
    }

    fn restart_blocked(&self, _cx: &App) -> Option<better_ui::restart::Blocker> {
        crate::restart::restart_blocked(
            self.transaction_running(),
            self.repair_task.is_some(),
            self.heal_task.is_some(),
            self.page == Page::Installing,
        )
    }

    fn restart_state(&self, _window: &Window, cx: &App) -> crate::restart::ManagerView {
        use crate::restart::{FilterKey, PageKey, TabKey};
        crate::restart::ManagerView {
            page: PageKey::of(&self.page),
            back: self.history.pages().iter().map(PageKey::of).collect(),
            detail_tab: TabKey::of(self.detail_tab),
            activity_filter: FilterKey::of(self.activity_filter),
            search: self.search_query.clone(),
            nav_search: self.nav_search.read(cx).value().to_string(),
            wide_collapsed: self.wide_collapsed,
            narrow_open: self.narrow_open,
            about_expanded: self.about_expanded,
            update_check_requested: self.update_check_requested,
            scroll: -f32::from(self.content_scroll.offset().y),
            pending_plan: self.pending_plan.clone(),
            finished_plan: self.finished_plan.clone(),
        }
    }
}

#[cfg(test)]
mod requested_component_tests {
    use super::{Page, appearance_of, opens_requested_component};
    use better_ui::fluent::theme::Appearance;
    use manager_core::StoredTheme;

    #[test]
    fn a_requested_page_never_replaces_an_operation() {
        assert!(opens_requested_component(&Page::Home, true));
        assert!(!opens_requested_component(&Page::Home, false));
        assert!(!opens_requested_component(&Page::Installing, true));
    }

    #[test]
    fn the_stored_appearance_is_the_kit_s_appearance() {
        assert_eq!(appearance_of(StoredTheme::Dark), Appearance::Dark);
        assert_eq!(appearance_of(StoredTheme::Light), Appearance::Light);
        assert_eq!(appearance_of(StoredTheme::System), Appearance::System);
    }
}

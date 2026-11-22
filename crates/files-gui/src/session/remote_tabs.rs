//! A tab on a server folder whose connection is not there (ticket 310):
//! connecting it, joining an attempt already in flight, cancelling, the
//! page each outcome leaves, and coming back to the same folder, selection
//! and scroll when a dropped connection returns.
//!
//! Nothing here waits. The reader decides, off the window's thread, that a
//! listing needs its connection ([`crate::remote::need`] and its probe), and
//! says so with [`ListingError::NotConnected`] or
//! [`ListingError::NotAnswering`]; [`FilesSession::pump_remote`] sees that
//! in the tab's model and connects it. Every attempt runs on its own thread
//! ([`crate::remote::Attempts`]).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use files_core::{EntryId, ListingError, ListingStatus, Location, TabId};

use crate::network::{Ask, MountError, MountFailure};
use crate::remote::{
    Attempts, HostState, MOUNT_LIST_AGE, MountWatch, Mounted, MountsSource, Need, RETRY_EVERY,
    Reason, Remote,
};

use super::{FilesSession, Notice};

/// What a tab whose folder is on a server shows instead of the folder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinkState {
    /// Connecting; `again` when the connection dropped while the folder was
    /// open.
    Connecting { again: bool },
    /// The connection did not come about.
    Failed { reason: Reason, detail: String },
    /// The person cancelled: "Not connected to {server}" with Connect.
    Cancelled,
    /// The connection dropped and this computer is on another network now
    /// (or which one cannot be told): nothing is sent there by itself, and
    /// the page offers Connect.
    OtherNetwork,
    /// A folder under GVfs's folder whose mount name is not understood: its
    /// address has to be typed once.
    NeedsAddress,
}

/// What a tab was showing when its connection went, to come back to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavedView {
    pub selected: Vec<String>,
    pub cursor: Option<String>,
    pub scroll: Option<f32>,
}

/// One tab waiting on a server.
#[derive(Clone, Debug)]
pub struct TabLink {
    pub tab: TabId,
    /// The location the tab is on (what it was opened as).
    pub location: Location,
    /// The server folder, `None` for [`LinkState::NeedsAddress`].
    pub remote: Option<Remote>,
    pub state: LinkState,
    pub saved: SavedView,
    /// A paste waiting for the connection.
    paste: Option<crate::commands::Clipboard>,
    /// When it started waiting, for a dropped connection's own limit.
    since: Instant,
    /// A dropped connection, or a server that stopped answering, is tried
    /// again by itself at this time.
    retry_at: Option<Instant>,
    /// The connection is there and the folder did not answer: tried again
    /// by listing, not by mounting.
    answering_only: bool,
    /// A favourite written as a GVfs folder whose mount name was not
    /// understood, to be rewritten by the address typed in this tab.
    replaces: Option<PathBuf>,
    /// How many times the tab's pane had read its folder when the wait
    /// began: a reading that completes after that ends the wait.
    readings: u64,
    /// The connection dropped while the folder was open: it is tried again
    /// by itself, for [`RemoteSide::retry_limit`] of time with a network,
    /// while the network is the one the folder was open on.
    dropped: bool,
    /// The network the folder was open on.
    network: crate::remote::Network,
    /// Since when there has been no network: that time does not count
    /// against the retry limit.
    paused_at: Option<Instant>,
    /// When GVfs itself is next asked to connect while a look at the host
    /// cannot tell whether it is there.
    gvfs_try_at: Option<Instant>,
}

/// A paste whose connection came: it goes in once the folder is listed,
/// and is never dropped without a word.
#[derive(Clone, Debug)]
pub(crate) struct PendingPaste {
    tab: TabId,
    location: Location,
    clipboard: crate::commands::Clipboard,
}

/// Why a held paste was not done.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PasteWhy {
    /// The tab left the folder, or closed, before it connected.
    Left,
    /// The folder is not on the server any more.
    Gone,
    /// The folder could not be listed.
    Failed,
    /// The connection was cancelled, or will not be made by itself.
    Cancelled,
}

impl TabLink {
    pub fn server(&self) -> String {
        self.remote.as_ref().map(Remote::server).unwrap_or_default()
    }

    pub fn key(&self) -> Option<String> {
        self.remote.as_ref().map(Remote::mount_key)
    }
}

/// The session's side of server folders.
pub(crate) struct RemoteSide {
    pub(crate) gvfs_root: Option<PathBuf>,
    pub(crate) watch: MountWatch,
    pub(crate) attempts: Attempts,
    pub(crate) links: Vec<TabLink>,
    /// Questions asked by attempts, waiting for their tab to be in front.
    pub(crate) questions: Vec<Ask>,
    /// Whether this computer has a network; a test says.
    pub(crate) network_up: Arc<dyn Fn() -> bool + Send + Sync>,
    /// The last good view of each tab on a server folder.
    pub(crate) good: Vec<(TabId, Location, SavedView)>,
    pub(crate) seen_mounts: u64,
    /// Nearest-folder searches running, by tab.
    pub(crate) climbing: Vec<(TabId, std::sync::mpsc::Receiver<Option<PathBuf>>)>,
    /// The scroll to put back on the front tab once it is listed.
    pub(crate) scroll_to_restore: Option<f32>,
    /// The front tab's scroll, as the window last said it.
    pub(crate) scroll_now: f32,
    /// The address typed on the Address-needed page, and the mount folder
    /// of the favourite it is for.
    pub(crate) typed_for: Option<(Location, PathBuf)>,
    /// Looks at a host before a dropped connection is tried again.
    pub(crate) reach: Arc<crate::remote::HostCheck>,
    /// The network this computer is on, read off the window's thread.
    pub(crate) network: crate::remote::NetworkWatch,
    /// The network each tab's server folder was last listed on.
    pub(crate) open_on: Vec<(TabId, crate::remote::Network)>,
    /// How long after a drop it is tried again by itself.
    pub(crate) retry_limit: Duration,
    /// How long a retrying wait says "connecting again" before it says why
    /// (while it goes on trying).
    pub(crate) says_why_after: Duration,
    /// Looks at hosts running, by tab.
    pub(crate) reach_checks: Vec<(TabId, std::sync::mpsc::Receiver<HostState>)>,
    /// Pastes whose connection came, waiting for their folder's listing.
    pub(crate) pending_pastes: Vec<PendingPaste>,
}

impl RemoteSide {
    pub(crate) fn new(
        gvfs_root: Option<PathBuf>,
        mounts: Arc<dyn MountsSource>,
        mounter: Arc<dyn crate::network::Mounter>,
    ) -> Self {
        Self {
            gvfs_root,
            watch: MountWatch::new(mounts),
            attempts: Attempts::new(mounter),
            links: Vec::new(),
            questions: Vec::new(),
            network_up: Arc::new(crate::remote::network_up),
            good: Vec::new(),
            seen_mounts: 0,
            climbing: Vec::new(),
            scroll_to_restore: None,
            scroll_now: 0.0,
            typed_for: None,
            reach: Arc::new(crate::remote::host_state),
            network: crate::remote::NetworkWatch::new(),
            open_on: Vec::new(),
            retry_limit: RETRY_LIMIT,
            says_why_after: DROPPED_LIMIT,
            reach_checks: Vec::new(),
            pending_pastes: Vec::new(),
        }
    }
}

/// How long a dropped connection keeps saying "connecting again" before it
/// says why, while it goes on trying.
const DROPPED_LIMIT: Duration = crate::network::MOUNT_TIMEOUT;

/// How long after a drop it is tried again by itself, counting only time
/// with a network (a computer asleep or off Wi-Fi waits as long as it
/// takes); after that, only a click connects. Long enough for a NAS or a
/// server to restart.
pub const RETRY_LIMIT: Duration = Duration::from_secs(600);

/// How often GVfs itself is asked to connect a dropped server while a look
/// at the host cannot tell whether it is there (an `ssh_config` alias, a
/// jump host, a NetBIOS name).
const GVFS_RETRY_EVERY: Duration = Duration::from_secs(30);

/// How many times, and how often, a folder a server says is not there is
/// looked at again before the nearest folder above it is opened.
const CLIMB_RECHECKS: u32 = 10;
const CLIMB_RECHECK_EVERY: Duration = Duration::from_millis(500);

/// How soon a folder that was slow to answer is listed again the first time.
const FIRST_RELIST: Duration = Duration::from_secs(1);

impl FilesSession {
    // --- What the window reads ---------------------------------------------

    /// The front tab's wait on a server, when it has one.
    pub fn link(&self) -> Option<&TabLink> {
        let tab = self.active_tab();
        self.remote.links.iter().find(|link| link.tab == tab)
    }

    /// Any tab's wait, for tests.
    pub fn link_of(&self, tab: TabId) -> Option<&TabLink> {
        self.remote.links.iter().find(|link| link.tab == tab)
    }

    /// Whether any server is being connected to.
    pub fn is_mounting(&self) -> bool {
        !self.remote.attempts.is_empty()
    }

    /// GVfs's folder, as this window knows it.
    pub fn gvfs_root(&self) -> Option<&Path> {
        self.remote.gvfs_root.as_deref()
    }

    /// Whether a favourite's or a row's server is connected: `None` for a
    /// location that is not on a server, or a mount list not read yet.
    pub fn connected(&self, location: &Location) -> Option<bool> {
        let root = self.remote.gvfs_root.as_deref();
        let remote = crate::remote::remote_of(location, root)?;
        let mounts = self.remote.watch.current();
        mounts.known.as_ref()?;
        Some(mounts.find(&remote).is_some())
    }

    /// The folder a server location is shown under now, when its mount is
    /// there: what a favourite's row lights for.
    pub fn shown_at(&self, location: &Location) -> Option<Location> {
        match crate::remote::need(
            location,
            self.remote.gvfs_root.as_deref(),
            &self.remote.watch.current(),
        ) {
            Need::At(local) => Some(Location::Local(local)),
            _ => None,
        }
    }

    /// The form a location is pinned and compared in: a folder under GVfs's
    /// folder is its server's address.
    pub fn stored_form(&self, location: &Location) -> Location {
        crate::remote::stored_form(location, self.remote.gvfs_root.as_deref())
    }

    /// Whether `location` is pinned, in whichever form it is.
    pub fn is_pinned(&self, location: &Location) -> bool {
        self.bookmarks.contains(&self.stored_form(location))
    }

    /// The window says how far the front tab is scrolled, so a dropped
    /// connection comes back to it.
    pub fn note_scroll(&mut self, offset: f32) {
        self.remote.scroll_now = offset;
    }

    /// The scroll to put back, once, after a dropped connection returned.
    pub fn take_scroll_to_restore(&mut self) -> Option<f32> {
        if self.is_listing() {
            return None;
        }
        self.remote.scroll_to_restore.take()
    }

    /// Uses another way of mounting servers, and of reading the mount list,
    /// and another GVfs folder. For tests.
    pub fn set_remote(
        &mut self,
        gvfs_root: Option<PathBuf>,
        mounts: Arc<dyn MountsSource>,
        mounter: Arc<dyn crate::network::Mounter>,
    ) {
        self.remote = RemoteSide::new(gvfs_root, mounts, mounter);
    }

    /// Uses another way of mounting servers. For tests.
    pub fn set_mounter(&mut self, mounter: Arc<dyn crate::network::Mounter>) {
        self.remote.attempts.set_mounter(mounter);
    }

    /// Says whether this computer has a network. For tests.
    pub fn set_network_up(&mut self, up: Arc<dyn Fn() -> bool + Send + Sync>) {
        self.remote.network_up = up;
    }

    /// Says how a host looks, before a retry and after a failure. For tests.
    pub fn set_host_check(&mut self, check: Arc<crate::remote::HostCheck>) {
        self.remote.reach = check;
    }

    /// Says which network this computer is on. For tests.
    pub fn set_network(&mut self, network: Arc<dyn Fn() -> crate::remote::Network + Send + Sync>) {
        self.remote.network = crate::remote::NetworkWatch::direct(network);
    }

    /// How long after a drop it is tried again by itself. For tests.
    pub fn set_retry_limit(&mut self, limit: Duration) {
        self.remote.retry_limit = limit;
    }

    /// How long a retrying wait says "connecting again" before it says why.
    /// For tests.
    pub fn set_says_why_after(&mut self, after: Duration) {
        self.remote.says_why_after = after;
    }

    // --- What the person does ---------------------------------------------

    /// Opens a server address typed in the path bar, clicked as a machine,
    /// or named by a plugin: the tab goes to it and connects.
    pub fn mount_server(&mut self, address: String) {
        match Location::parse_uri(&address) {
            location @ Location::Network(_) => {
                // What was said about the folder left behind is not about
                // this one.
                self.notice = None;
                self.navigate_to(location);
            }
            _ => {
                self.notice = Some(Notice::MountFailed {
                    address,
                    error: MountError::new(MountFailure::NotSupported, ""),
                })
            }
        }
    }

    /// An address typed in the address bar: when the front tab shows the
    /// Address-needed page for a favourite under GVfs's folder, that
    /// address is the one its favourites take once it connects.
    pub(crate) fn note_address_typed(&mut self, address: &str) {
        let tab = self.active_tab();
        let root = self.remote.gvfs_root.clone();
        self.remote.typed_for = self
            .remote
            .links
            .iter()
            .find(|link| link.tab == tab && link.state == LinkState::NeedsAddress)
            .and_then(|link| match (&link.location, root.as_deref()) {
                (Location::Local(path), Some(root)) => {
                    crate::remote::split_gvfs_path(path.as_path(), root)
                        .map(|(name, _)| root.join(name))
                }
                _ => None,
            })
            .map(|mount| (Location::parse_uri(address), mount));
    }

    /// Cancel on the connecting page or the sign-in: every tab waiting on
    /// that server is left "Not connected".
    pub fn cancel_connecting(&mut self) {
        let Some(key) = self.link().and_then(TabLink::key) else {
            return;
        };
        self.remote.attempts.cancel(&key);
        self.withdraw_questions(&key);
        self.leave_not_connected(&key);
    }

    /// Every tab waiting on `key`'s server is left "Not connected": no
    /// retry, and a paste it held is said not to be done.
    fn leave_not_connected(&mut self, key: &str) {
        let tabs: Vec<TabId> = self
            .remote
            .links
            .iter()
            .filter(|link| link.key().as_deref() == Some(key))
            .map(|link| link.tab)
            .collect();
        for tab in tabs {
            self.leave_tab(tab, LinkState::Cancelled);
        }
    }

    /// One tab left in `state` (not connected, or on another network): no
    /// retry, and a paste it held or that waited for its listing is said
    /// not to be done, now, never run later by surprise.
    fn leave_tab(&mut self, tab: TabId, state: LinkState) {
        let Some(link) = self.remote.links.iter_mut().find(|link| link.tab == tab) else {
            return;
        };
        link.state = state;
        link.retry_at = None;
        link.dropped = false;
        let mut lost = link.paste.take().map(|_| link.location.clone());
        let (theirs, others): (Vec<_>, Vec<_>) = std::mem::take(&mut self.remote.pending_pastes)
            .into_iter()
            .partition(|paste| paste.tab == tab);
        self.remote.pending_pastes = others;
        if let Some(paste) = theirs.into_iter().next() {
            lost = lost.or(Some(paste.location));
        }
        if let Some(location) = lost {
            self.paste_not_done(&location, PasteWhy::Cancelled);
        }
    }

    /// Says a held paste into `location` was not done, and why.
    fn paste_not_done(&mut self, location: &Location, why: PasteWhy) {
        let remote = crate::remote::remote_of(location, self.remote.gvfs_root.as_deref());
        self.notice = Some(Notice::PasteNotDone {
            folder: remote
                .as_ref()
                .map(Remote::folder_name)
                .unwrap_or_else(|| location.display_name()),
            server: remote.map(|remote| remote.server()).unwrap_or_default(),
            why,
        });
    }

    /// Try again, or Connect after a cancel.
    pub fn retry_connecting(&mut self) {
        let tab = self.active_tab();
        // A click: connected now, on whatever network this is, and not
        // tried again by itself after.
        if let Some(link) = self.remote.links.iter_mut().find(|link| link.tab == tab) {
            link.dropped = false;
            link.paused_at = None;
            link.since = Instant::now();
        }
        self.reconnect(tab);
    }

    /// Network settings on a failure page.
    pub fn open_network_settings(&mut self, installed: &dyn Fn(&str) -> bool) {
        if let Some((program, arguments)) = crate::remote::network_settings(installed) {
            let invocation = app_catalog_core::Invocation { program, arguments };
            if self.spawner.spawn(&invocation).is_err() {
                self.notice = Some(Notice::NoTerminal);
            }
        }
    }

    /// Sends the person's answer to the question shown, and takes it off
    /// the screen until GVfs asks the next one or the mount ends.
    pub fn answer_server(&mut self, answer: crate::network::Answer) {
        if let Some(ask) = self.server_prompt.take() {
            let cancelled = matches!(answer, crate::network::Answer::Cancel);
            let key = crate::remote::key_of_address(&ask.address);
            let _ = ask.reply.try_send(answer);
            // Cancelling the sign-in cancels the connection for every tab
            // waiting on it.
            if cancelled && let Some(key) = key {
                self.remote.attempts.cancel(&key);
                self.withdraw_questions(&key);
                self.leave_not_connected(&key);
            }
        }
    }

    /// Cancels the question shown, if any.
    pub fn cancel_server_prompt(&mut self) {
        if self.server_prompt.is_some() {
            self.answer_server(crate::network::Answer::Cancel);
        }
    }

    /// Paste into a folder whose server is not connected: it waits for the
    /// connection. Returns whether it was held.
    pub(crate) fn hold_paste(&mut self) -> bool {
        let tab = self.active_tab();
        let location = self.location().clone();
        let needs = self.remote.links.iter().any(|link| link.tab == tab)
            || matches!(
                crate::remote::need(
                    &location,
                    self.remote.gvfs_root.as_deref(),
                    &self.remote.watch.current()
                ),
                Need::Connect(_)
            );
        if !needs || self.clipboard.is_empty() {
            return false;
        }
        let clipboard = self.clipboard.clone();
        if !self.remote.links.iter().any(|link| link.tab == tab) {
            self.begin_link(tab, &location, false, None);
        }
        if let Some(link) = self.remote.links.iter_mut().find(|link| link.tab == tab) {
            link.paste = Some(clipboard);
            if !matches!(link.state, LinkState::Connecting { .. }) {
                self.reconnect(tab);
            }
        }
        true
    }

    /// The front tab goes somewhere (not a reload): the folder it goes to is
    /// opened, never "dropped", even when it is the one it showed a moment
    /// ago.
    pub(crate) fn forget_open_folder(&mut self) {
        let tab = self.active_tab();
        self.remote.good.retain(|(id, _, _)| *id != tab);
    }

    // --- The rule, applied --------------------------------------------------

    /// After a navigation: a location the rule already knows needs its
    /// connection starts connecting now, without waiting for the listing
    /// to say so; a server address whose mount is there goes straight to
    /// its folder. Any wait the tab had for another location ends.
    pub(crate) fn link_after_navigation(&mut self) {
        let tab = self.active_tab();
        let location = self.location().clone();
        // Only an address typed on the Address-needed page, for the folder
        // that page was about, may later rewrite favourites (ticket 310's
        // review): any other navigation leaves them alone.
        let replaces = self
            .remote
            .typed_for
            .take()
            .filter(|(typed, _)| *typed == location)
            .map(|(_, mount)| mount);
        // Leaving a folder that held a paste: the paste is said not to be
        // done, now, and never runs later by surprise.
        let mut left = Vec::new();
        self.remote.links.retain(|link| {
            let keep = link.tab != tab || link.location == location;
            if !keep && link.paste.is_some() {
                left.push(link.location.clone());
            }
            keep
        });
        for folder in left {
            self.paste_not_done(&folder, PasteWhy::Left);
        }
        if let Some(link) = self.remote.links.iter().find(|link| link.tab == tab) {
            // Opening the folder again (its favourite, the address) while it
            // is not connected is a click on Connect.
            if matches!(
                link.state,
                LinkState::Cancelled | LinkState::OtherNetwork | LinkState::Failed { .. }
            ) {
                self.retry_connecting();
            }
            return;
        }
        match crate::remote::need(
            &location,
            self.remote.gvfs_root.as_deref(),
            &self.remote.watch.current(),
        ) {
            Need::At(local) => {
                // The address typed on the Address-needed page is of a server
                // already mounted: its favourites take it now.
                if let (Some(replaced), Some(remote)) = (
                    &replaces,
                    crate::remote::remote_of(&location, self.remote.gvfs_root.as_deref()),
                ) {
                    let mount = remote.at(None);
                    self.edit_bookmarks(|file| {
                        (crate::remote::migrate_under(file, replaced, &mount), ())
                    });
                }
                let reader = self.reader.clone();
                self.pane_mut()
                    .replace_current(Location::Local(local), reader.as_ref());
                self.sync_history(tab);
            }
            Need::Connect(_) => self.begin_link(tab, &location, false, replaces),
            Need::Address(_) | Need::Nothing => {}
        }
    }

    /// Starts a tab's wait for `location`'s server, and connects (or joins
    /// the attempt in flight).
    fn begin_link(
        &mut self,
        tab: TabId,
        location: &Location,
        again: bool,
        replaces: Option<PathBuf>,
    ) {
        let remote = crate::remote::remote_of(location, self.remote.gvfs_root.as_deref());
        let saved = self.saved_view(tab, location);
        let state = match &remote {
            Some(_) => LinkState::Connecting { again },
            None => LinkState::NeedsAddress,
        };
        let readings = self.readings_of(tab);
        let held = self.held_paste(tab, location);
        self.remote.links.retain(|link| link.tab != tab);
        self.remote.links.push(TabLink {
            tab,
            location: location.clone(),
            remote: remote.clone(),
            state,
            saved,
            paste: held,
            since: Instant::now(),
            retry_at: None,
            answering_only: false,
            replaces,
            readings,
            dropped: false,
            network: crate::remote::Network::default(),
            paused_at: None,
            gvfs_try_at: None,
        });
        if let Some(remote) = remote {
            self.remote.attempts.connect(&remote);
        }
    }

    /// The network `tab`'s folder was last listed on, or the one this
    /// computer is on now when it has one.
    fn network_of(&self, tab: TabId) -> crate::remote::Network {
        self.remote
            .open_on
            .iter()
            .find(|(id, network)| *id == tab && !network.is_none())
            .map(|(_, network)| network.clone())
            .unwrap_or_else(|| self.remote.network.current())
    }

    /// A connection that went while its folder was open: the tab says it is
    /// connecting again and comes back by itself (ticket 310's second
    /// review). GVfs says nothing that tells a mount let go elsewhere from
    /// a server that went away (both are the tracker's `Unmounted` after
    /// the backend's `UnregisterMount`; the unmount request itself goes
    /// over GVfs's private connection), and Better Files unmounts no server
    /// itself, so every one is a drop: the owner asked for the folder to
    /// come back after sleep, a Wi-Fi drop or a restarted server.
    fn begin_drop(&mut self, tab: TabId, location: &Location) {
        let remote = crate::remote::remote_of(location, self.remote.gvfs_root.as_deref());
        let saved = self.saved_view(tab, location);
        let readings = self.readings_of(tab);
        let paste = self
            .remote
            .links
            .iter()
            .find(|link| link.tab == tab)
            .and_then(|link| link.paste.clone());
        let network = self.network_of(tab);
        self.remote.links.retain(|link| link.tab != tab);
        self.remote.links.push(TabLink {
            tab,
            location: location.clone(),
            remote,
            state: LinkState::Connecting { again: true },
            saved,
            paste,
            since: Instant::now(),
            retry_at: None,
            answering_only: false,
            replaces: None,
            readings,
            dropped: true,
            network,
            paused_at: None,
            gvfs_try_at: None,
        });
        self.try_dropped(tab);
    }

    /// One try of a dropped connection. With no network at the moment it
    /// waits, and that time does not count; on the network the folder was
    /// open on, the host is looked at and the connection made; on another
    /// network, or one that cannot be told apart from another, the page
    /// says so and offers Connect.
    fn try_dropped(&mut self, tab: TabId) {
        let network = self.remote.network.current();
        let known = self.remote.network.known();
        let limit = (self.remote.retry_limit, self.remote.says_why_after);
        let Some(link) = self.remote.links.iter_mut().find(|link| link.tab == tab) else {
            return;
        };
        // A server on this computer is on no network: it always comes back
        // by itself.
        let on_this_computer = link
            .remote
            .as_ref()
            .is_some_and(Remote::is_on_this_computer);
        if !known && !on_this_computer {
            link.retry_at = Some(Instant::now() + FIRST_RELIST);
            return;
        }
        // The network went (it had a default route and has none now): wait,
        // and that time does not count. A folder opened with no default
        // route (a LAN with no gateway) carries on as before.
        if network.is_none() && !link.network.is_none() && !on_this_computer {
            let paused = *link.paused_at.get_or_insert_with(Instant::now);
            link.retry_at = Some(Instant::now() + RETRY_EVERY);
            // Past its first 30 s the page says why, while it waits on.
            if paused.elapsed() >= limit.1 && matches!(link.state, LinkState::Connecting { .. }) {
                link.state = LinkState::Failed {
                    reason: Reason::NoNetwork,
                    detail: String::new(),
                };
            }
            return;
        }
        if let Some(paused) = link.paused_at.take() {
            link.since += paused.elapsed();
        }
        if !on_this_computer && !link.network.same_as(&network) {
            self.leave_tab(tab, LinkState::OtherNetwork);
            return;
        }
        if waited(link) >= limit.0 {
            // Past the limit: only a click tries again.
            schedule_retry(link, limit);
        } else if link.answering_only {
            // The mount is there: listed again, nothing is sent but that.
            self.reconnect(tab);
        } else {
            self.look_at_host(tab);
        }
    }

    /// Looks at the tab's host on a thread of its own.
    fn look_at_host(&mut self, tab: TabId) {
        if self.remote.reach_checks.iter().any(|(id, _)| *id == tab) {
            return;
        }
        let Some(remote) = self
            .remote
            .links
            .iter()
            .find(|link| link.tab == tab)
            .and_then(|link| link.remote.clone())
        else {
            return;
        };
        let reach = Arc::clone(&self.remote.reach);
        let (sender, receiver) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("files-remote-host".to_string())
            .spawn(move || {
                let _ = sender.send(reach(&remote));
            });
        if spawned.is_ok() {
            self.remote.reach_checks.push((tab, receiver));
        }
    }

    /// What the looks at hosts found. A host that answers is connected to
    /// at once (a restarted server). One the look cannot reach is not taken
    /// for gone: a look from here does not see an `ssh_config` alias, a
    /// jump host or a NetBIOS name, so GVfs itself is asked every
    /// [`GVFS_RETRY_EVERY`]; in between the host is looked at again.
    fn finish_host_looks(&mut self) -> bool {
        let mut found = Vec::new();
        self.remote
            .reach_checks
            .retain(|(tab, receiver)| match receiver.try_recv() {
                Ok(state) => {
                    found.push((*tab, state));
                    false
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => true,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
            });
        let changed = !found.is_empty();
        let limit = (self.remote.retry_limit, self.remote.says_why_after);
        for (tab, state) in found {
            let Some(link) = self
                .remote
                .links
                .iter_mut()
                .find(|link| link.tab == tab && link.dropped)
            else {
                continue;
            };
            let Some(remote) = link.remote.clone() else {
                continue;
            };
            let now = Instant::now();
            let ask_gvfs =
                state == HostState::Reachable || link.gvfs_try_at.is_none_or(|at| now >= at);
            if ask_gvfs && !self.remote.attempts.is_connecting(&remote.mount_key()) {
                if state != HostState::Reachable {
                    link.gvfs_try_at = Some(now + GVFS_RETRY_EVERY);
                }
                self.remote.attempts.connect(&remote);
            }
            if let Some(link) = self.remote.links.iter_mut().find(|link| link.tab == tab) {
                schedule_retry(link, limit);
            }
        }
        changed
    }

    /// The paste a tab's wait for `location` holds, carried into the wait
    /// that replaces it.
    fn held_paste(&self, tab: TabId, location: &Location) -> Option<crate::commands::Clipboard> {
        self.remote
            .links
            .iter()
            .find(|link| link.tab == tab && link.location == *location)
            .and_then(|link| link.paste.clone())
    }

    fn readings_of(&self, tab: TabId) -> u64 {
        self.panes
            .iter()
            .find(|(id, _)| *id == tab)
            .map_or(0, |(_, pane)| pane.readings())
    }

    /// Tries a tab's connection again: by listing when the mount is there
    /// and only the folder did not answer, by connecting otherwise.
    fn reconnect(&mut self, tab: TabId) {
        let Some(link) = self.remote.links.iter_mut().find(|link| link.tab == tab) else {
            return;
        };
        let Some(remote) = link.remote.clone() else {
            return;
        };
        let again =
            matches!(link.state, LinkState::Connecting { again: true }) || link.retry_at.is_some();
        link.state = LinkState::Connecting { again };
        link.retry_at = None;
        if link.answering_only {
            link.answering_only = false;
            let saved = link.saved.clone();
            let reader = self.reader.clone();
            if let Some((_, pane)) = self.panes.iter_mut().find(|(id, _)| *id == tab) {
                pane.reload(reader.as_ref());
                // Listed again in place: when it answers, the same
                // selection and scroll as before it stopped.
                restore_selection(pane.model_mut(), &saved);
            }
            if tab == self.active_tab() {
                self.remote.scroll_to_restore = saved.scroll.filter(|scroll| *scroll > 0.0);
            }
        } else {
            self.remote.attempts.connect(&remote);
        }
    }

    /// The selection a tab had the last time its folder was listed, and the
    /// scroll the window last said, when it is the same folder.
    fn saved_view(&self, tab: TabId, location: &Location) -> SavedView {
        let mut saved = self
            .remote
            .good
            .iter()
            .find(|(id, at, _)| *id == tab && at == location)
            .map(|(_, _, view)| view.clone())
            .unwrap_or_default();
        if saved.selected.is_empty()
            && let Some((_, pane)) = self.panes.iter().find(|(id, _)| *id == tab)
        {
            // A selection set before the listing came back (a file handed
            // in, a restart's) is kept too.
            saved = view_of(pane.model());
        }
        if tab == self.active_tab() {
            saved.scroll = Some(self.remote.scroll_now);
        }
        saved
    }

    /// Everything a frame does for server folders. Returns whether anything
    /// the window draws changed.
    pub(crate) fn pump_remote(&mut self) -> bool {
        let mut changed = false;
        // The mount list is read again every few seconds on its own thread
        // while GVfs's folder is known: the sidebar's captions, a recent
        // file's folder and a dropped connection are all read from it.
        if self.remote.gvfs_root.is_some() {
            self.remote.watch.refresh_if_older(MOUNT_LIST_AGE);
        }
        changed |= self.follow_listings();
        changed |= self.settle_pastes();
        changed |= self.follow_mount_list();
        changed |= self.take_questions();
        changed |= self.land_attempts();
        changed |= self.retry_due();
        changed |= self.finish_host_looks();
        changed |= self.finish_climbs();
        changed
    }

    /// Reads each tab's listing: remembers a good one's view, and starts a
    /// wait for one that needs its connection.
    fn follow_listings(&mut self) -> bool {
        let root = self.remote.gvfs_root.clone();
        let mut needing: Vec<(TabId, Location, ListingError)> = Vec::new();
        // A folder counts as open only while its tab stays on it: one the
        // person comes back to is opened, not dropped.
        let panes = &self.panes;
        self.remote.good.retain(|(tab, at, _)| {
            panes
                .iter()
                .any(|(id, pane)| id == tab && pane.location() == at)
        });
        let mut listed_now: Vec<TabId> = Vec::new();
        for (tab, pane) in &self.panes {
            let location = pane.location();
            if !crate::remote::is_remote(location, root.as_deref()) {
                continue;
            }
            match pane.model().status() {
                ListingStatus::Complete => {
                    listed_now.push(*tab);
                    let view = view_of(pane.model());
                    match self.remote.good.iter_mut().find(|(id, _, _)| id == tab) {
                        Some(entry) => {
                            entry.1 = location.clone();
                            entry.2 = view;
                        }
                        None => self.remote.good.push((*tab, location.clone(), view)),
                    }
                }
                ListingStatus::Failed(error) => {
                    needing.push((*tab, location.clone(), error.clone()));
                }
                _ => {}
            }
        }
        // The network each open server folder is on, to tell after a drop
        // whether this is still it. A reading with no network says nothing.
        if !listed_now.is_empty() {
            let network = self.remote.network.current();
            let panes = &self.panes;
            self.remote
                .open_on
                .retain(|(tab, _)| panes.iter().any(|(id, _)| id == tab));
            if !network.is_none() {
                for tab in listed_now {
                    self.remote.open_on.retain(|(id, _)| *id != tab);
                    self.remote.open_on.push((tab, network.clone()));
                }
            }
        }
        // A listing begun after the wait that came back ends it, and so does
        // leaving the folder or closing the tab.
        let panes = &self.panes;
        let before = self.remote.links.len();
        let mut ended: Vec<TabLink> = Vec::new();
        self.remote.links.retain(|link| {
            let keep = panes
                .iter()
                .find(|(id, _)| *id == link.tab)
                .is_some_and(|(_, pane)| {
                    pane.location() == &link.location
                        && !(matches!(pane.model().status(), ListingStatus::Complete)
                            && !pane.is_listing()
                            && !pane.is_refreshing()
                            && pane.readings() > link.readings)
                });
            if !keep {
                ended.push(link.clone());
            }
            keep
        });
        // A paste the wait held is never lost without a word: one whose
        // folder came back by its listing goes in now; one whose tab left
        // says so.
        for link in ended {
            let Some(clipboard) = link.paste else {
                continue;
            };
            let there = self
                .panes
                .iter()
                .any(|(id, pane)| *id == link.tab && pane.location() == &link.location);
            if there {
                self.remote.pending_pastes.push(PendingPaste {
                    tab: link.tab,
                    location: link.location.clone(),
                    clipboard,
                });
            } else {
                self.paste_not_done(&link.location, PasteWhy::Left);
            }
        }
        let mut changed = before != self.remote.links.len();
        for (tab, location, error) in needing {
            let existing = self
                .remote
                .links
                .iter()
                .find(|link| link.tab == tab && link.location == location)
                .map(|link| {
                    // A dropped connection past its first 30 s shows why
                    // while it goes on trying: its listings are still its
                    // tries (seen stalled in CI when they were ignored).
                    if link.dropped {
                        LinkState::Connecting { again: true }
                    } else {
                        link.state.clone()
                    }
                });
            let was_open = self
                .remote
                .good
                .iter()
                .any(|(id, at, _)| *id == tab && *at == location);
            match error {
                ListingError::NotConnected { .. } => match existing {
                    // A folder that was open: a drop. Anything else: the
                    // person opened it, and it connects now.
                    None if was_open => {
                        self.begin_drop(tab, &location);
                        changed = true;
                    }
                    None => {
                        self.begin_link(tab, &location, false, None);
                        changed = true;
                    }
                    // Waiting by listing again, and the mount turned out to
                    // be gone: a drop looks at the host first; anything else
                    // connects.
                    Some(LinkState::Connecting { .. }) => {
                        if let Some(link) =
                            self.remote.links.iter_mut().find(|link| link.tab == tab)
                            && let Some(remote) = link.remote.clone()
                            && !self.remote.attempts.is_connecting(&remote.mount_key())
                            && link.retry_at.is_none()
                        {
                            link.answering_only = false;
                            if link.dropped {
                                self.look_at_host(tab);
                            } else {
                                self.remote.attempts.connect(&remote);
                            }
                            changed = true;
                        }
                    }
                    // The page already says why.
                    Some(_) => {}
                },
                ListingError::NotAnswering { .. } => match existing {
                    None => {
                        self.begin_link_answering(tab, &location, was_open);
                        changed = true;
                    }
                    Some(LinkState::Connecting { .. }) => {
                        self.schedule_answering(tab);
                        changed = true;
                    }
                    Some(_) => {}
                },
                ListingError::NotFound { .. } if existing.is_none() || was_open => {
                    if self.remote.climbing.iter().all(|(id, _)| *id != tab)
                        && let Location::Local(path) = &location
                    {
                        self.climb(tab, path.as_path().to_path_buf());
                        changed = true;
                    }
                }
                ListingError::NotADirectory { .. } => {
                    // A file handed in on a server: its folder, with it
                    // selected.
                    if let Location::Local(path) = &location
                        && let Some(parent) = path.parent()
                    {
                        let name = path.file_name();
                        let reader = self.reader.clone();
                        if let Some((_, pane)) = self.panes.iter_mut().find(|(id, _)| *id == tab) {
                            pane.replace_current(Location::Local(parent), reader.as_ref());
                            pane.model_mut()
                                .selection_mut()
                                .select_only(EntryId::Name(name));
                        }
                        self.sync_history(tab);
                        changed = true;
                    }
                }
                _ => {}
            }
        }
        changed
    }

    /// A tab whose server is mounted and whose folder did not answer: it
    /// says it is connecting again and lists again by itself.
    fn begin_link_answering(&mut self, tab: TabId, location: &Location, again: bool) {
        let remote = crate::remote::remote_of(location, self.remote.gvfs_root.as_deref());
        let saved = self.saved_view(tab, location);
        let readings = self.readings_of(tab);
        let held = self.held_paste(tab, location);
        let network = self.network_of(tab);
        self.remote.links.retain(|link| link.tab != tab);
        self.remote.links.push(TabLink {
            tab,
            location: location.clone(),
            remote,
            state: LinkState::Connecting { again },
            saved,
            paste: held,
            since: Instant::now(),
            // Its mount is there and its folder was slow to answer: it is
            // listed again by itself (no connection is made, nothing is
            // sent but the listing), soon at first, then within the retry
            // limit. A slow server is not a failure.
            retry_at: Some(Instant::now() + FIRST_RELIST),
            answering_only: true,
            replaces: None,
            readings,
            dropped: true,
            network,
            paused_at: None,
            gvfs_try_at: None,
        });
    }

    fn schedule_answering(&mut self, tab: TabId) {
        let limit = (self.remote.retry_limit, self.remote.says_why_after);
        if let Some(link) = self.remote.links.iter_mut().find(|link| link.tab == tab) {
            link.answering_only = true;
            if link.retry_at.is_none() && link.dropped {
                schedule_retry(link, limit);
            }
        }
    }

    /// A mount that left GVfs's list while a tab showed a folder on it: the
    /// tab connects again, in place.
    fn follow_mount_list(&mut self) -> bool {
        let generation = self.remote.watch.generation();
        if generation == self.remote.seen_mounts {
            return false;
        }
        self.remote.seen_mounts = generation;
        let mounts = self.remote.watch.current();
        let root = self.remote.gvfs_root.clone();
        let mut dropped = Vec::new();
        for (tab, pane) in &self.panes {
            // Only a folder that was open and listed can drop; one just
            // opened is connected by its own listing.
            let open = self
                .remote
                .good
                .iter()
                .any(|(id, at, _)| id == tab && at == pane.location());
            if !open || self.remote.links.iter().any(|link| link.tab == *tab) {
                continue;
            }
            if let Need::Connect(_) = crate::remote::need(pane.location(), root.as_deref(), &mounts)
                && matches!(pane.location(), Location::Local(_))
            {
                dropped.push((*tab, pane.location().clone()));
            }
        }
        let changed = !dropped.is_empty();
        for (tab, location) in dropped {
            self.begin_drop(tab, &location);
        }
        changed
    }

    /// The watch hub says a folder on a server is gone: unless the server
    /// says so too, that is the connection, not the folder. Returns whether
    /// it was taken as a dropped connection.
    pub(crate) fn remote_folder_gone(&mut self, tab: TabId) -> bool {
        let root = self.remote.gvfs_root.clone();
        let Some((_, pane)) = self.panes.iter().find(|(id, _)| *id == tab) else {
            return false;
        };
        let location = pane.location().clone();
        if !crate::remote::is_remote(&location, root.as_deref()) {
            return false;
        }
        if !self.remote.links.iter().any(|link| link.tab == tab) {
            self.begin_link_answering(tab, &location, true);
            if let Some(link) = self.remote.links.iter_mut().find(|link| link.tab == tab) {
                link.retry_at = None;
                link.answering_only = false;
            }
            // Listed again now: the probe tells whether the mount went
            // (connect), the folder went (the nearest folder above), or the
            // server stopped answering (try again).
            let reader = self.reader.clone();
            if let Some((_, pane)) = self.panes.iter_mut().find(|(id, _)| *id == tab) {
                pane.reload(reader.as_ref());
            }
        }
        self.remote.watch.refresh();
        true
    }

    /// Takes the questions attempts asked, and shows the one for the front
    /// tab's server.
    fn take_questions(&mut self) -> bool {
        let mut changed = false;
        // GVfs withdrew the question shown (it sent `Aborted`, or the mount
        // was given up on): it leaves the screen.
        if self
            .server_prompt
            .as_ref()
            .is_some_and(|ask| ask.reply.is_closed())
        {
            self.server_prompt = None;
            changed = true;
        }
        let asked = self.remote.attempts.questions();
        changed |= !asked.is_empty();
        self.remote.questions.extend(asked);
        self.remote.questions.retain(|ask| !ask.reply.is_closed());
        // A question nobody waits for any more is answered "cancel".
        let attempts = &self.remote.attempts;
        let (keep, orphaned): (Vec<Ask>, Vec<Ask>) = std::mem::take(&mut self.remote.questions)
            .into_iter()
            .partition(|ask| {
                crate::remote::key_of_address(&ask.address)
                    .is_some_and(|key| attempts.is_connecting(&key))
            });
        for ask in orphaned {
            let _ = ask.reply.try_send(crate::network::Answer::Cancel);
        }
        self.remote.questions = keep;
        // The front tab's question is shown; one shown for a tab that went
        // to the back waits again.
        let front = self.link().and_then(TabLink::key);
        if let Some(ask) = self.server_prompt.take() {
            if crate::remote::key_of_address(&ask.address) == front {
                self.server_prompt = Some(ask);
            } else {
                self.remote.questions.insert(0, ask);
                changed = true;
            }
        }
        if self.server_prompt.is_none()
            && let Some(front) = front
            && let Some(at) = self.remote.questions.iter().position(|ask| {
                crate::remote::key_of_address(&ask.address).as_deref() == Some(front.as_str())
            })
        {
            self.server_prompt = Some(self.remote.questions.remove(at));
            self.server_prompt_serial += 1;
            changed = true;
        }
        changed
    }

    fn withdraw_questions(&mut self, key: &str) {
        let mine = |ask: &Ask| crate::remote::key_of_address(&ask.address).as_deref() == Some(key);
        if self.server_prompt.as_ref().is_some_and(mine)
            && let Some(ask) = self.server_prompt.take()
        {
            let _ = ask.reply.try_send(crate::network::Answer::Cancel);
        }
        let (gone, keep): (Vec<Ask>, Vec<Ask>) = std::mem::take(&mut self.remote.questions)
            .into_iter()
            .partition(mine);
        for ask in gone {
            let _ = ask.reply.try_send(crate::network::Answer::Cancel);
        }
        self.remote.questions = keep;
    }

    /// Each attempt that ended: every tab waiting on it goes to its folder,
    /// or says why not.
    fn land_attempts(&mut self) -> bool {
        let finished = self.remote.attempts.finished();
        if finished.is_empty() {
            return false;
        }
        for (key, result) in finished {
            if self.server_prompt.as_ref().is_some_and(|ask| {
                crate::remote::key_of_address(&ask.address).as_deref() == Some(key.as_str())
            }) {
                self.server_prompt = None;
            }
            match result {
                Ok(mounted) => self.land(&key, &mounted),
                Err(error) => self.refuse(&key, &error),
            }
        }
        self.remote.watch.refresh();
        true
    }

    fn land(&mut self, key: &str, mounted: &Mounted) {
        let waiting: Vec<TabLink> = self
            .remote
            .links
            .iter()
            .filter(|link| link.key().as_deref() == Some(key))
            // A dropped connection that shows why while it tries again
            // behind the page lands too.
            .filter(|link| matches!(link.state, LinkState::Connecting { .. }) || link.dropped)
            .cloned()
            .collect();
        if let Some(remote) = waiting.first().and_then(|link| link.remote.clone()) {
            let address = remote.mount_address();
            self.server_folders.retain(|(known, _)| *known != address);
            self.server_folders.push((address, mounted.root.clone()));
        }
        // The mount is new: a probe stuck on the dead one speaks for it no more.
        self.reader.forget_probe(&mounted.root);
        let active = self.active_tab();
        let reader = self.reader.clone();
        for link in waiting {
            let Some(remote) = &link.remote else {
                continue;
            };
            let Some(target) = remote.local_in(&mounted.root, &mounted.folder) else {
                continue;
            };
            let target = Location::Local(target);
            if let Some(replaced) = &link.replaces {
                let mount = remote.at(None);
                self.edit_bookmarks(|file| {
                    (crate::remote::migrate_under(file, replaced, &mount), ())
                });
            }
            self.remote.links.retain(|known| known.tab != link.tab);
            let Some((_, pane)) = self.panes.iter_mut().find(|(id, _)| *id == link.tab) else {
                continue;
            };
            // A selection made while it waited (a file handed in, a recent
            // file's folder) is the one to come back to.
            let mut saved = link.saved.clone();
            if saved.selected.is_empty() {
                let waiting = view_of(pane.model());
                saved.selected = waiting.selected;
                saved.cursor = waiting.cursor;
            }
            if pane.location() == &target {
                pane.reload(reader.as_ref());
            } else {
                pane.replace_current(target.clone(), reader.as_ref());
            }
            restore_selection(pane.model_mut(), &saved);
            self.sync_history(link.tab);
            if link.tab == active {
                self.remote.scroll_to_restore = link.saved.scroll.filter(|scroll| *scroll > 0.0);
                self.after_landing();
            }
            // A held paste goes in once the folder is listed and known to be
            // there: never into a folder deleted on the server meanwhile.
            if let Some(clipboard) = link.paste {
                self.remote.pending_pastes.push(PendingPaste {
                    tab: link.tab,
                    location: target.clone(),
                    clipboard,
                });
            }
        }
    }

    /// The held pastes whose folder is listed now go in; one whose folder
    /// is gone, could not be listed, or was left says so.
    fn settle_pastes(&mut self) -> bool {
        let pending = std::mem::take(&mut self.remote.pending_pastes);
        let before = pending.len();
        for paste in pending {
            let pane = self
                .panes
                .iter()
                .find(|(id, _)| *id == paste.tab)
                .map(|(_, pane)| pane);
            let Some(pane) = pane.filter(|pane| pane.location() == &paste.location) else {
                self.paste_not_done(&paste.location, PasteWhy::Left);
                continue;
            };
            match pane.model().status() {
                ListingStatus::Complete if !pane.is_listing() => self.run_paste(paste),
                ListingStatus::Failed(ListingError::NotFound { .. }) => {
                    self.paste_not_done(&paste.location, PasteWhy::Gone)
                }
                ListingStatus::Failed(
                    ListingError::NotConnected { .. } | ListingError::NotAnswering { .. },
                )
                | ListingStatus::Loading
                | ListingStatus::Complete => self.remote.pending_pastes.push(paste),
                _ => self.paste_not_done(&paste.location, PasteWhy::Failed),
            }
        }
        self.remote.pending_pastes.len() != before
    }

    /// Pastes a held clipboard into its folder. A held cut empties the
    /// clipboard only when it is still that cut: a newer copy stays.
    fn run_paste(&mut self, paste: PendingPaste) {
        match crate::commands::paste(&paste.clipboard, &paste.location) {
            Ok(spec) => {
                self.submit(spec);
                if matches!(paste.clipboard, crate::commands::Clipboard::Cut(_))
                    && self.clipboard == paste.clipboard
                {
                    self.clipboard = crate::commands::Clipboard::Empty;
                }
            }
            Err(refusal) => self.notice = Some(Notice::Command(refusal)),
        }
    }

    fn refuse(&mut self, key: &str, error: &MountError) {
        let network_up = (self.remote.network_up)();
        let limit = (self.remote.retry_limit, self.remote.says_why_after);
        for link in &mut self.remote.links {
            // A dropped connection past its first 30 s shows why while it
            // goes on trying: its tries are still its own.
            if link.key().as_deref() != Some(key)
                || !(matches!(link.state, LinkState::Connecting { .. }) || link.dropped)
            {
                continue;
            }
            if error.failure == MountFailure::Cancelled {
                link.state = LinkState::Cancelled;
                continue;
            }
            let reason = Reason::of(error, &link.server(), network_up);
            // A connection that dropped keeps trying by itself within its
            // limit, and says "connecting again" for its first 30 s. A
            // refused sign-in and a changed host key are never tried again
            // by themselves, and say so at once.
            let final_reason = matches!(
                reason,
                Reason::SignInRefused | Reason::Mount(MountFailure::HostKeyChanged)
            );
            if link.dropped && !final_reason {
                schedule_retry(link, limit);
                // Past its first 30 s the page says why, in this failure's
                // own words, while it goes on trying.
                if matches!(link.state, LinkState::Failed { .. }) {
                    link.state = LinkState::Failed {
                        reason,
                        detail: error.detail.trim().to_string(),
                    };
                }
                continue;
            }
            link.retry_at = None;
            link.dropped = false;
            link.state = LinkState::Failed {
                reason,
                detail: error.detail.trim().to_string(),
            };
        }
    }

    /// Dropped connections are tried again when their time comes.
    fn retry_due(&mut self) -> bool {
        let due: Vec<TabId> = self
            .remote
            .links
            .iter()
            .filter(|link| link.retry_at.is_some_and(|at| Instant::now() >= at))
            .map(|link| link.tab)
            .collect();
        let changed = !due.is_empty();
        for tab in due {
            let Some(link) = self.remote.links.iter_mut().find(|link| link.tab == tab) else {
                continue;
            };
            let failed = link.state.clone();
            link.retry_at = None;
            self.try_dropped(tab);
            // A failure page stays up while it tries again behind it.
            if let LinkState::Failed { reason, detail } = failed
                && let Some(link) = self.remote.links.iter_mut().find(|link| link.tab == tab)
                && matches!(link.state, LinkState::Connecting { .. })
            {
                link.state = LinkState::Failed { reason, detail };
            }
        }
        changed
    }

    /// The folder a tab showed is gone on a server that answered: the
    /// nearest folder above it that is there is found off this thread.
    fn climb(&mut self, tab: TabId, from: PathBuf) {
        let root = self.remote.gvfs_root.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        let deadline = crate::remote::PROBE_DEADLINE;
        let probing = crate::remote::Probing::default();
        let spawned = std::thread::Builder::new()
            .name("files-remote-climb".to_string())
            .spawn(move || {
                let Some(root) = root else {
                    let _ = sender.send(None);
                    return;
                };
                let Some((name, _)) = crate::remote::split_gvfs_path(&from, &root) else {
                    let _ = sender.send(None);
                    return;
                };
                let mount = root.join(name);
                // GVfs's FUSE folder can say "not found" for a moment about
                // a folder that is there, right after a mount is made again
                // or as a connection drops (seen in CI, ticket 310's second
                // round): the folder itself is looked at again before
                // anything above it is opened.
                for _ in 0..CLIMB_RECHECKS {
                    std::thread::sleep(CLIMB_RECHECK_EVERY);
                    if crate::remote::probe(&from, &mount, deadline, &probing)
                        == crate::remote::Probed::Folder
                    {
                        let _ = sender.send(Some(from));
                        return;
                    }
                }
                let mut at = from.parent().map(Path::to_path_buf);
                while let Some(folder) = at {
                    if !folder.starts_with(&mount) {
                        break;
                    }
                    if crate::remote::probe(&folder, &mount, deadline, &probing)
                        == crate::remote::Probed::Folder
                    {
                        let _ = sender.send(Some(folder));
                        return;
                    }
                    at = folder.parent().map(Path::to_path_buf);
                }
                let _ = sender.send(None);
            });
        if spawned.is_ok() {
            self.remote.climbing.push((tab, receiver));
        }
    }

    fn finish_climbs(&mut self) -> bool {
        let mut found = Vec::new();
        self.remote
            .climbing
            .retain(|(tab, receiver)| match receiver.try_recv() {
                Ok(folder) => {
                    found.push((*tab, folder));
                    false
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => true,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
            });
        let changed = !found.is_empty();
        let root = self.remote.gvfs_root.clone();
        for (tab, folder) in found {
            let Some((_, pane)) = self.panes.iter_mut().find(|(id, _)| *id == tab) else {
                continue;
            };
            let gone = pane.location().clone();
            let Some(folder) = folder.and_then(|folder| Location::local(folder).ok()) else {
                // Not even the mount's own folder is there: that is the
                // connection, not the folder. The tab connects again rather
                // than stay on "not found" with nothing waiting.
                if !self.remote.links.iter().any(|link| link.tab == tab) {
                    self.begin_link(tab, &gone, false, None);
                }
                continue;
            };
            let reader = self.reader.clone();
            // The folder was there after all: listed again, in place.
            if folder == gone {
                pane.reload(reader.as_ref());
                continue;
            }
            pane.replace_current(folder, reader.as_ref());
            self.remote.links.retain(|link| link.tab != tab);
            self.sync_history(tab);
            // "Nothing was pasted" says more than "the folder above was
            // opened", and stays.
            if tab == self.active_tab() && !matches!(self.notice, Some(Notice::PasteNotDone { .. }))
            {
                let remote = crate::remote::remote_of(&gone, root.as_deref());
                self.notice = Some(Notice::RemoteFolderGone {
                    folder: remote
                        .as_ref()
                        .map(Remote::folder_name)
                        .unwrap_or_else(|| gone.display_name()),
                    server: remote.map(|remote| remote.server()).unwrap_or_default(),
                });
                self.resync_content();
            }
        }
        changed
    }
}

#[cfg(test)]
impl FilesSession {
    pub(crate) fn open_entry_for_tests(&mut self, entry: &files_core::Entry) {
        self.open_entry(entry);
    }

    /// Makes the mount shown at `mount_folder` look like one whose last
    /// probe has not come back.
    /// The waits, attempts and looks at hosts, for a test's failure message.
    #[cfg(test)]
    pub(crate) fn describe_remote_for_tests(&self) -> String {
        let now = Instant::now();
        let waits: Vec<String> = self
            .remote
            .links
            .iter()
            .map(|link| {
                format!(
                    "{:?} dropped={} answering_only={} waited={:?} retry_in={:?} gvfs_in={:?} paused={:?} network={:?}",
                    link.state,
                    link.dropped,
                    link.answering_only,
                    waited(link),
                    link.retry_at.map(|at| at.saturating_duration_since(now)),
                    link.gvfs_try_at.map(|at| at.saturating_duration_since(now)),
                    link.paused_at.map(|at| at.elapsed()),
                    link.network,
                )
            })
            .collect();
        format!(
            "waits [{}], attempts [{}], host looks out {}, network now {:?} (known {})",
            waits.join(" | "),
            self.remote.attempts.describe(),
            self.remote.reach_checks.len(),
            self.remote.network.current(),
            self.remote.network.known()
        )
    }

    /// A search for the nearest folder above the front tab's that found
    /// nothing (not even the mount's own folder answered). For tests.
    #[cfg(test)]
    pub(crate) fn climb_found_nothing_for_tests(&mut self) {
        let tab = self.active_tab();
        let (sender, receiver) = std::sync::mpsc::channel();
        let _ = sender.send(None);
        self.remote.climbing.push((tab, receiver));
    }

    /// A paste whose connection came, waiting for `location`'s listing in
    /// the front tab. For tests.
    #[cfg(test)]
    pub(crate) fn pend_paste_for_tests(
        &mut self,
        location: &Location,
        clipboard: crate::commands::Clipboard,
    ) {
        let tab = self.active_tab();
        self.remote.pending_pastes.push(PendingPaste {
            tab,
            location: location.clone(),
            clipboard,
        });
    }

    pub(crate) fn hold_probe_for_tests(&self, mount_folder: &Path) {
        self.reader.hold_probe_for_tests(mount_folder);
    }

    pub(crate) fn release_probe_for_tests(&self, mount_folder: &Path) {
        self.reader.release_probe_for_tests(mount_folder);
    }
}

impl crate::sidebar::RemoteState for FilesSession {
    fn connection(&self, location: &Location) -> Option<Option<bool>> {
        crate::remote::is_remote(location, self.remote.gvfs_root.as_deref())
            .then(|| self.connected(location))
    }
}

/// A model's selection, by name.
fn view_of(model: &files_core::DirectoryModel) -> SavedView {
    let name = |id: &EntryId| match id {
        EntryId::Name(name) => Some(name.clone()),
        _ => None,
    };
    let selection = model.selection();
    SavedView {
        selected: selection.ids().filter_map(name).collect(),
        cursor: selection.cursor().and_then(name),
        scroll: None,
    }
}

/// The next try of a dropped connection: in [`RETRY_EVERY`], while within
/// `limit` of the drop; past it, the page says the server did not answer
/// and only a click tries again. Past its first 30 s the page says why
/// while it goes on trying.
fn schedule_retry(link: &mut TabLink, (limit, says_why_after): (Duration, Duration)) {
    let no_answer = LinkState::Failed {
        reason: Reason::NoAnswer,
        detail: String::new(),
    };
    if waited(link) >= limit {
        link.retry_at = None;
        link.dropped = false;
        if matches!(link.state, LinkState::Connecting { .. }) {
            link.state = no_answer;
        }
        return;
    }
    link.retry_at = Some(Instant::now() + RETRY_EVERY);
    if waited(link) >= says_why_after && matches!(link.state, LinkState::Connecting { .. }) {
        link.state = no_answer;
    }
}

/// How long a dropped connection has been tried with a network: the time
/// with none (asleep, off Wi-Fi) does not count.
fn waited(link: &TabLink) -> Duration {
    let all = link.since.elapsed();
    match link.paused_at {
        Some(paused) => all.saturating_sub(paused.elapsed()),
        None => all,
    }
}

/// Puts a saved selection back on a model being listed: names are kept
/// until their entries arrive.
fn restore_selection(model: &mut files_core::DirectoryModel, saved: &SavedView) {
    let selection = model.selection_mut();
    let mut first = true;
    for name in &saved.selected {
        if first {
            selection.select_only(EntryId::Name(name.clone()));
            first = false;
        } else {
            selection.toggle(EntryId::Name(name.clone()));
        }
    }
    if let Some(cursor) = &saved.cursor {
        selection.move_cursor(Some(EntryId::Name(cursor.clone())));
    }
}

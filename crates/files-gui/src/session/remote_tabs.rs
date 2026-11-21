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
    /// by itself, within [`RemoteSide::retry_limit`], while the network is
    /// the one it dropped on.
    dropped: bool,
    /// The host was looked at once since the drop.
    checked_once: bool,
    /// The network (`remote::network_id`) when it dropped.
    network: String,
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
    /// Names the network this computer is on.
    pub(crate) network_id: Arc<dyn Fn() -> String + Send + Sync>,
    /// How long after a drop it is tried again by itself.
    pub(crate) retry_limit: Duration,
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
            network_id: Arc::new(crate::remote::network_id),
            retry_limit: RETRY_LIMIT,
            reach_checks: Vec::new(),
            pending_pastes: Vec::new(),
        }
    }
}

/// How long a dropped connection keeps saying "connecting again" before it
/// says why, while it goes on trying.
const DROPPED_LIMIT: Duration = crate::network::MOUNT_TIMEOUT;

/// How long after a drop it is tried again by itself; after that, only a
/// click connects.
pub const RETRY_LIMIT: Duration = Duration::from_secs(120);

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
        self.remote.reach = Arc::clone(&check);
        self.remote.attempts.set_host_check(check);
    }

    /// Says which network this computer is on. For tests.
    pub fn set_network_id(&mut self, id: Arc<dyn Fn() -> String + Send + Sync>) {
        self.remote.network_id = id;
    }

    /// How long after a drop it is tried again by itself. For tests.
    pub fn set_retry_limit(&mut self, limit: Duration) {
        self.remote.retry_limit = limit;
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
        let mut dropped = Vec::new();
        for link in &mut self.remote.links {
            if link.key().as_deref() == Some(key) {
                link.state = LinkState::Cancelled;
                link.retry_at = None;
                link.dropped = false;
                if link.paste.take().is_some() {
                    dropped.push(link.location.clone());
                }
            }
        }
        for location in dropped {
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
        // A click: connected now, and not tried again by itself after.
        if let Some(link) = self.remote.links.iter_mut().find(|link| link.tab == tab) {
            link.dropped = false;
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
        self.remote
            .links
            .retain(|link| link.tab != tab || link.location == location);
        if self.remote.links.iter().any(|link| link.tab == tab) {
            return;
        }
        match crate::remote::need(
            &location,
            self.remote.gvfs_root.as_deref(),
            &self.remote.watch.current(),
        ) {
            Need::At(local) => {
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
            checked_once: false,
            network: String::new(),
        });
        if let Some(remote) = remote {
            self.remote.attempts.connect(&remote);
        }
    }

    /// A connection that went while its folder was open: the tab says it is
    /// connecting again, and the host is looked at first (ticket 310's
    /// review). A server that answers at once was let go on purpose
    /// (unmounted elsewhere): the tab is left "Not connected", and nothing
    /// is mounted again without a click. One that does not answer is tried
    /// again by itself, within the retry limit, on the same network.
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
            checked_once: false,
            network: (self.remote.network_id)(),
        });
        self.look_at_host(tab);
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

    /// What the looks at hosts found: connect, wait and look again, or stop.
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
        let network = (self.remote.network_id)();
        for (tab, state) in found {
            let Some(link) = self.remote.links.iter_mut().find(|link| link.tab == tab) else {
                continue;
            };
            let first = !link.checked_once;
            link.checked_once = true;
            let Some(key) = link.key() else {
                continue;
            };
            if link.network != network || (first && state == HostState::Reachable) {
                // Another network, or a server that let the mount go while
                // answering: only a click connects.
                self.leave_tab_not_connected(tab);
            } else if state == HostState::Reachable {
                if let Some(remote) = link.remote.clone()
                    && !self.remote.attempts.is_connecting(&key)
                {
                    self.remote.attempts.connect(&remote);
                }
            } else {
                schedule_retry(link, self.remote.retry_limit);
            }
        }
        changed
    }

    /// One tab left "Not connected", its held paste said not done.
    fn leave_tab_not_connected(&mut self, tab: TabId) {
        let Some(link) = self.remote.links.iter_mut().find(|link| link.tab == tab) else {
            return;
        };
        link.state = LinkState::Cancelled;
        link.retry_at = None;
        link.dropped = false;
        if link.paste.take().is_some() {
            let location = link.location.clone();
            self.paste_not_done(&location, PasteWhy::Cancelled);
        }
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
        for (tab, pane) in &self.panes {
            let location = pane.location();
            if !crate::remote::is_remote(location, root.as_deref()) {
                continue;
            }
            match pane.model().status() {
                ListingStatus::Complete => {
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
                .map(|link| link.state.clone());
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
            checked_once: false,
            network: (self.remote.network_id)(),
        });
    }

    fn schedule_answering(&mut self, tab: TabId) {
        let limit = self.remote.retry_limit;
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
            .filter(|link| matches!(link.state, LinkState::Connecting { .. }))
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
        let limit = self.remote.retry_limit;
        for link in &mut self.remote.links {
            if link.key().as_deref() != Some(key)
                || !matches!(link.state, LinkState::Connecting { .. })
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
        let network = (self.remote.network_id)();
        for tab in due {
            let Some(link) = self.remote.links.iter_mut().find(|link| link.tab == tab) else {
                continue;
            };
            let failed = link.state.clone();
            link.retry_at = None;
            // On another network nothing is tried by itself: the name may
            // now be someone else's.
            if link.network != network {
                self.leave_tab_not_connected(tab);
                continue;
            }
            if link.answering_only {
                self.reconnect(tab);
            } else {
                // The host is looked at before any connection is made.
                self.look_at_host(tab);
            }
            // A failure page stays up while it tries again behind it.
            if let LinkState::Failed { reason, detail } = failed
                && let Some(link) = self.remote.links.iter_mut().find(|link| link.tab == tab)
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
                continue;
            };
            let reader = self.reader.clone();
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
fn schedule_retry(link: &mut TabLink, limit: Duration) {
    let no_answer = LinkState::Failed {
        reason: Reason::NoAnswer,
        detail: String::new(),
    };
    if link.since.elapsed() >= limit {
        link.retry_at = None;
        link.dropped = false;
        if matches!(link.state, LinkState::Connecting { .. }) {
            link.state = no_answer;
        }
        return;
    }
    link.retry_at = Some(Instant::now() + RETRY_EVERY);
    if link.since.elapsed() >= DROPPED_LIMIT && matches!(link.state, LinkState::Connecting { .. }) {
        link.state = no_answer;
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

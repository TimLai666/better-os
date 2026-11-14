//! The page registry: one entry per page of the control center, in the
//! navigation pane's order, and the states every page shares.
//!
//! A page is a module that says, without GPUI, what it is called, which
//! settings cards the search can find on it, what Home should show about it,
//! and what badge its pane item carries. The window draws the pane, the
//! search, and Home from this list alone, so a page ticket adds its module
//! and replaces one line of [`PAGES`] and touches nothing else in the shell.
//!
//! The pages tickets 85 to 90 build are registered now so the pane, deep
//! links, and search already know them: System, Touchpad, Keep awake, and
//! Fixes as placeholders, which a debug build draws as "not built yet", and
//! Apps and Default apps as today's Better Manager screens, shown inside the
//! new window until their tickets replace them. Neither kind may ship in a
//! release; ticket 91's release gate fails on any entry that is not
//! [`PageStatus::Built`].

use crate::attention::{self, AttentionItem, Snapshot};
use crate::i18n::{Locale, Shell, shell};
use crate::navigation::Route;
use crate::search::SearchEntry;
use crate::{home, settings_page, window_attention_page};

/// How far along a page is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PageStatus {
    /// The page its spec section describes.
    Built,
    /// Nothing yet; the named ticket builds it.
    Placeholder { ticket: u16 },
    /// Today's screens for it, kept working inside the control center until
    /// the named ticket rebuilds them.
    Interim { ticket: u16 },
}

/// What a page's pane item carries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NavBadge {
    /// "Something is going on here", as Keep awake shows while a session
    /// holds the machine awake. Ticket 87 is the first to give it.
    #[allow(dead_code)]
    Dot,
    /// How many things are waiting; never zero.
    Count(usize),
}

/// One page, as the shell knows it.
pub(crate) struct PageModule {
    pub(crate) route: Route,
    /// The name `better-manager --page` takes.
    pub(crate) name: &'static str,
    pub(crate) status: PageStatus,
    /// The page's title, which is also its pane label.
    pub(crate) title: fn(&'static Shell) -> &'static str,
    /// The settings cards the search finds on this page.
    pub(crate) search_entries: fn(Locale) -> Vec<SearchEntry>,
    /// What Home shows about this page.
    pub(crate) attention: fn(&Snapshot) -> Vec<AttentionItem>,
    pub(crate) badge: fn(&Snapshot) -> Option<NavBadge>,
}

fn no_entries(_: Locale) -> Vec<SearchEntry> {
    Vec::new()
}

fn no_attention(_: &Snapshot) -> Vec<AttentionItem> {
    Vec::new()
}

fn no_badge(_: &Snapshot) -> Option<NavBadge> {
    None
}

/// One of the three cards at the top of Apps that open its sub-pages.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppsLink {
    pub(crate) card: &'static str,
    pub(crate) header: &'static str,
    pub(crate) description: String,
    pub(crate) page: crate::model::Page,
}

/// The ids of Apps' sub-page cards.
pub(crate) mod apps_card {
    pub(crate) const UPDATES: &str = "apps-updates";
    pub(crate) const HEALTH: &str = "apps-health";
    pub(crate) const ACTIVITY: &str = "apps-activity";
}

/// Apps' sub-page cards (spec section 7): Updates with its count, Health,
/// and Activity. They are what reaches the update check, Update all, the
/// doctor, restore, and the activity log, since the pane no longer lists
/// those screens.
pub(crate) fn apps_links(locale: Locale, update_count: usize) -> Vec<AppsLink> {
    use crate::model::Page;
    let s = shell(locale);
    let updates = match update_count {
        0 => s.apps_up_to_date.to_string(),
        1 => s.apps_update_count_one.to_string(),
        n => s.apps_updates_count.replace("{n}", &n.to_string()),
    };
    vec![
        AppsLink {
            card: apps_card::UPDATES,
            header: s.apps_updates,
            description: updates,
            page: Page::Updates,
        },
        AppsLink {
            card: apps_card::HEALTH,
            header: s.apps_health,
            description: s.apps_health_description.to_string(),
            page: Page::Health,
        },
        AppsLink {
            card: apps_card::ACTIVITY,
            header: s.apps_activity,
            description: s.apps_activity_description.to_string(),
            page: Page::Activity,
        },
    ]
}

fn apps_entries(locale: Locale) -> Vec<SearchEntry> {
    let s = shell(locale);
    let entry = |header, description, card| SearchEntry {
        header,
        description,
        page: s.apps,
        group: None,
        route: Route::Apps,
        card,
    };
    vec![
        entry(s.apps_updates, "", apps_card::UPDATES),
        entry(s.apps_health, s.apps_health_description, apps_card::HEALTH),
        entry(
            s.apps_activity,
            s.apps_activity_description,
            apps_card::ACTIVITY,
        ),
    ]
}

/// Apps: today's screens until ticket 85, under its three sub-page cards.
/// Its Home items and its update badge exist now, from `manager-core`'s
/// state.
const APPS: PageModule = PageModule {
    route: Route::Apps,
    name: "apps",
    status: PageStatus::Interim { ticket: 85 },
    title: |s| s.apps,
    search_entries: apps_entries,
    attention: attention::apps_attention,
    badge: |snapshot| {
        let count = snapshot.update_count();
        (count > 0).then_some(NavBadge::Count(count))
    },
};

const SYSTEM: PageModule = PageModule {
    route: Route::System,
    name: "system",
    status: PageStatus::Placeholder { ticket: 86 },
    title: |s| s.system,
    search_entries: no_entries,
    attention: no_attention,
    badge: no_badge,
};

const TOUCHPAD: PageModule = PageModule {
    route: Route::Touchpad,
    name: "touchpad",
    status: PageStatus::Placeholder { ticket: 88 },
    title: |s| s.touchpad,
    search_entries: no_entries,
    attention: no_attention,
    badge: no_badge,
};

const KEEP_AWAKE: PageModule = PageModule {
    route: Route::KeepAwake,
    name: "keep-awake",
    status: PageStatus::Placeholder { ticket: 87 },
    title: |s| s.keep_awake,
    search_entries: no_entries,
    attention: no_attention,
    badge: no_badge,
};

/// Default apps: today's Defaults screens until ticket 89.
const DEFAULT_APPS: PageModule = PageModule {
    route: Route::DefaultApps,
    name: "default-apps",
    status: PageStatus::Interim { ticket: 89 },
    title: |s| s.default_apps,
    search_entries: no_entries,
    attention: no_attention,
    badge: no_badge,
};

const FIXES: PageModule = PageModule {
    route: Route::Fixes,
    name: "fixes",
    status: PageStatus::Placeholder { ticket: 90 },
    title: |s| s.fixes,
    search_entries: no_entries,
    attention: no_attention,
    badge: no_badge,
};

/// The registration list, in the pane's order. One line per page.
pub(crate) static PAGES: [PageModule; 9] = [
    home::PAGE,
    SYSTEM,
    window_attention_page::PAGE,
    TOUCHPAD,
    KEEP_AWAKE,
    APPS,
    DEFAULT_APPS,
    FIXES,
    settings_page::PAGE,
];

/// The registered page for a pane item.
pub(crate) fn module(route: Route) -> &'static PageModule {
    PAGES
        .iter()
        .find(|page| page.route == route)
        .expect("every route has a registered page")
}

/// A page's title in a language.
pub(crate) fn title(route: Route, locale: Locale) -> &'static str {
    (module(route).title)(shell(locale))
}

/// The page registered under a `--page` name.
pub(crate) fn by_name(name: &str) -> Option<&'static PageModule> {
    PAGES.iter().find(|page| page.name == name)
}

/// Every page's search entries, in the pane's order.
pub(crate) fn search_index(locale: Locale) -> Vec<SearchEntry> {
    PAGES
        .iter()
        .flat_map(|page| (page.search_entries)(locale))
        .collect()
}

/// Every page's attention items, in Home's order.
pub(crate) fn attention(snapshot: &Snapshot) -> Vec<AttentionItem> {
    attention::order(
        PAGES
            .iter()
            .flat_map(|page| (page.attention)(snapshot))
            .collect(),
    )
}

/// Whether a debug build is running, which is the only build that draws a
/// placeholder's "not built yet" notice.
pub(crate) fn shows_not_built_notice() -> bool {
    cfg!(debug_assertions)
}

/// The states every page and card uses (spec section 2), with their copy,
/// so each page ticket draws them the same way. No page built so far has a
/// component, a service, or hardware behind it, so nothing outside the tests
/// reads this until tickets 85–90 do.
#[allow(dead_code)]
pub(crate) mod states {
    use crate::i18n::{Locale, shell};

    /// How long the first read may take before the page shows a ring.
    pub(crate) const LOADING_RING_AFTER_MS: u64 = 300;

    /// Which kit control draws a state.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Drawn {
        /// Nothing new: the previous content, or an empty page, stays.
        Nothing,
        /// A 32px `ProgressRing` under the title with Caption text beside it.
        ProgressRing,
        /// One `SettingsCard` with an accent button.
        Card,
        /// An `InfoBar` Warning above the cards, which are drawn disabled.
        WarningBar,
        /// An `InfoBar` Error.
        ErrorBar,
        /// An `ErrorBar`: one sentence and expandable, copyable details.
        ErrorReport,
        /// The card itself, disabled, with the sentence as its description.
        DisabledCard,
        /// The card is not drawn, nor its group header if the group is empty.
        Hidden,
    }

    /// A state a page or card is in.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub(crate) enum PageState {
        /// The first read is running; `elapsed_ms` since it started.
        Loading {
            elapsed_ms: u64,
        },
        NotInstalled {
            component: String,
        },
        TooOld {
            component: String,
            installed: String,
            required: String,
        },
        ServiceNotRunning {
            component: String,
        },
        HardwareAbsent,
        Unreadable,
        /// The person cancelled the password prompt or it failed.
        NoPermission,
        /// polkit says no for this session.
        NotAllowed,
        WriteFailed {
            reason: String,
        },
        ReadBackDifferent {
            value: String,
        },
    }

    /// How a state is drawn and what it says.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub(crate) struct StateCopy {
        pub(crate) drawn: Drawn,
        /// The sentence, or empty for a state with no words.
        pub(crate) text: String,
        /// The one button's label, when the state offers one.
        pub(crate) action: Option<&'static str>,
    }

    pub(crate) fn describe(locale: Locale, state: &PageState) -> StateCopy {
        let s = shell(locale);
        let copy = |drawn, text: String, action| StateCopy {
            drawn,
            text,
            action,
        };
        match state {
            PageState::Loading { elapsed_ms } if *elapsed_ms < LOADING_RING_AFTER_MS => {
                copy(Drawn::Nothing, String::new(), None)
            }
            PageState::Loading { .. } => {
                copy(Drawn::ProgressRing, s.state_loading.to_string(), None)
            }
            PageState::NotInstalled { component } => copy(
                Drawn::Card,
                s.state_not_installed.replace("{component}", component),
                Some(s.state_install),
            ),
            PageState::TooOld {
                component,
                installed,
                required,
            } => copy(
                Drawn::Card,
                s.state_too_old
                    .replace("{component}", component)
                    .replace("{installed}", installed)
                    .replace("{required}", required),
                Some(s.state_update),
            ),
            PageState::ServiceNotRunning { component } => copy(
                Drawn::WarningBar,
                s.state_service_stopped.replace("{component}", component),
                Some(s.state_start_service),
            ),
            PageState::HardwareAbsent => copy(Drawn::Hidden, String::new(), None),
            PageState::Unreadable => {
                copy(Drawn::DisabledCard, s.state_unreadable.to_string(), None)
            }
            PageState::NoPermission => {
                copy(Drawn::WarningBar, s.state_no_permission.to_string(), None)
            }
            PageState::NotAllowed => copy(Drawn::ErrorBar, s.state_not_allowed.to_string(), None),
            PageState::WriteFailed { reason } => copy(
                Drawn::ErrorReport,
                s.state_write_failed.replace("{reason}", reason),
                None,
            ),
            PageState::ReadBackDifferent { value } => copy(
                Drawn::WarningBar,
                s.state_read_back.replace("{value}", value),
                None,
            ),
        }
    }

    /// "Start service" is offered only for an installed, enabled component:
    /// starting a service the person turned off would undo their choice.
    pub(crate) fn offers_start_service(installed: bool, enabled: bool) -> bool {
        installed && enabled
    }

    /// A group header is drawn only over at least one card.
    pub(crate) fn draws_group_header(visible_cards: usize) -> bool {
        visible_cards > 0
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn text(locale: Locale, state: PageState) -> String {
            describe(locale, &state).text
        }

        #[test]
        fn a_short_read_draws_nothing_and_a_long_one_a_ring() {
            let quick = describe(Locale::EnUs, &PageState::Loading { elapsed_ms: 120 });
            assert_eq!(quick.drawn, Drawn::Nothing);
            assert!(quick.text.is_empty());
            let slow = describe(Locale::ZhTw, &PageState::Loading { elapsed_ms: 300 });
            assert_eq!(slow.drawn, Drawn::ProgressRing);
            assert_eq!(slow.text, "正在讀取…");
            assert_eq!(
                text(Locale::EnUs, PageState::Loading { elapsed_ms: 900 }),
                "Reading…"
            );
        }

        #[test]
        fn every_state_says_the_spec_sentence_in_both_languages() {
            let awake = || "Better Awake".to_string();
            let cases: Vec<(PageState, &str, &str)> = vec![
                (
                    PageState::NotInstalled { component: awake() },
                    "需要安裝 Better Awake 才能使用這一頁。",
                    "Better Awake needs to be installed to use this page.",
                ),
                (
                    PageState::TooOld {
                        component: awake(),
                        installed: "0.2.9".to_string(),
                        required: "0.3.0".to_string(),
                    },
                    "這一頁需要較新版本的 Better Awake。目前是 0.2.9，需要 0.3.0 以上。",
                    "This page needs a newer Better Awake. You have 0.2.9; it needs 0.3.0 or later.",
                ),
                (
                    PageState::ServiceNotRunning { component: awake() },
                    "Better Awake 的背景服務沒有在執行，這一頁暫時無法讀取或變更。",
                    "Better Awake's background service isn't running, so this page can't read or change anything right now.",
                ),
                (
                    PageState::Unreadable,
                    "這台電腦有這項功能，但目前讀不到它的狀態。",
                    "This PC has this, but its state can't be read right now.",
                ),
                (
                    PageState::NoPermission,
                    "沒有變更：身分驗證沒有完成。",
                    "Nothing changed: authentication wasn't completed.",
                ),
                (
                    PageState::NotAllowed,
                    "這個帳號沒有權限變更這項設定。",
                    "This account isn't allowed to change this setting.",
                ),
                (
                    PageState::WriteFailed {
                        reason: "The service did not answer.".to_string(),
                    },
                    "沒有變更成功。The service did not answer.",
                    "That didn't change. The service did not answer.",
                ),
                (
                    PageState::ReadBackDifferent {
                        value: "100%".to_string(),
                    },
                    "已寫入，但讀回來的值是 100%。",
                    "It was written, but it reads back as 100%.",
                ),
            ];
            for (state, zh, en) in cases {
                assert_eq!(text(Locale::ZhTw, state.clone()), zh);
                assert_eq!(text(Locale::EnUs, state), en);
            }
        }

        #[test]
        fn each_state_is_drawn_with_its_kit_control_and_its_one_action() {
            let drawn = |state| describe(Locale::EnUs, &state);
            let not_installed = drawn(PageState::NotInstalled {
                component: "Better Touchpad".to_string(),
            });
            assert_eq!(not_installed.drawn, Drawn::Card);
            assert_eq!(not_installed.action, Some("Install"));
            let too_old = drawn(PageState::TooOld {
                component: "x".to_string(),
                installed: "1".to_string(),
                required: "2".to_string(),
            });
            assert_eq!(
                (too_old.drawn, too_old.action),
                (Drawn::Card, Some("Update"))
            );
            let stopped = describe(
                Locale::ZhTw,
                &PageState::ServiceNotRunning {
                    component: "x".to_string(),
                },
            );
            assert_eq!(
                (stopped.drawn, stopped.action),
                (Drawn::WarningBar, Some("啟動服務"))
            );
            assert_eq!(drawn(PageState::HardwareAbsent).drawn, Drawn::Hidden);
            assert_eq!(drawn(PageState::Unreadable).drawn, Drawn::DisabledCard);
            assert_eq!(drawn(PageState::NoPermission).drawn, Drawn::WarningBar);
            assert_eq!(drawn(PageState::NotAllowed).drawn, Drawn::ErrorBar);
            assert_eq!(
                drawn(PageState::WriteFailed {
                    reason: String::new()
                })
                .drawn,
                Drawn::ErrorReport
            );
            assert_eq!(
                drawn(PageState::ReadBackDifferent {
                    value: String::new()
                })
                .drawn,
                Drawn::WarningBar
            );
        }

        #[test]
        fn start_service_needs_an_installed_enabled_component() {
            assert!(offers_start_service(true, true));
            assert!(!offers_start_service(true, false));
            assert!(!offers_start_service(false, true));
            assert!(draws_group_header(1));
            assert!(!draws_group_header(0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search;

    #[test]
    fn every_pane_item_has_exactly_one_page_in_the_pane_order() {
        let routes: Vec<Route> = PAGES.iter().map(|page| page.route).collect();
        assert_eq!(routes, Route::ALL);
        for route in Route::ALL {
            assert_eq!(module(route).route, route);
        }
    }

    #[test]
    fn page_names_are_the_spec_names_and_unique() {
        let names: Vec<&str> = PAGES.iter().map(|page| page.name).collect();
        assert_eq!(
            names,
            [
                "home",
                "system",
                "window-attention",
                "touchpad",
                "keep-awake",
                "apps",
                "default-apps",
                "fixes",
                "settings"
            ]
        );
        for name in names {
            assert_eq!(by_name(name).map(|page| page.name), Some(name));
        }
        assert!(by_name("updates").is_none());
        assert!(by_name("bogus").is_none());
    }

    #[test]
    fn titles_are_the_pane_labels_in_both_languages() {
        let en: Vec<&str> = Route::ALL
            .iter()
            .map(|route| title(*route, Locale::EnUs))
            .collect();
        assert_eq!(
            en,
            [
                "Home",
                "System",
                "Window attention",
                "Touchpad",
                "Keep awake",
                "Apps",
                "Default apps",
                "Fixes",
                "Settings"
            ]
        );
        let zh: Vec<&str> = Route::ALL
            .iter()
            .map(|route| title(*route, Locale::ZhTw))
            .collect();
        assert_eq!(
            zh,
            [
                "首頁",
                "系統",
                "視窗提醒",
                "觸控板",
                "保持清醒",
                "應用程式",
                "預設應用程式",
                "修正",
                "設定"
            ]
        );
    }

    /// The pages tickets 85–90 build are registered and say which ticket owns
    /// them, so ticket 91's release gate can refuse to ship them.
    #[test]
    fn the_unbuilt_pages_name_their_tickets() {
        let status = |route| module(route).status;
        assert_eq!(status(Route::Home), PageStatus::Built);
        assert_eq!(status(Route::Settings), PageStatus::Built);
        assert_eq!(status(Route::WindowAttention), PageStatus::Built);
        assert_eq!(status(Route::Apps), PageStatus::Interim { ticket: 85 });
        assert_eq!(
            status(Route::System),
            PageStatus::Placeholder { ticket: 86 }
        );
        assert_eq!(
            status(Route::KeepAwake),
            PageStatus::Placeholder { ticket: 87 }
        );
        assert_eq!(
            status(Route::Touchpad),
            PageStatus::Placeholder { ticket: 88 }
        );
        assert_eq!(
            status(Route::DefaultApps),
            PageStatus::Interim { ticket: 89 }
        );
        assert_eq!(status(Route::Fixes), PageStatus::Placeholder { ticket: 90 });
        assert_eq!(shows_not_built_notice(), cfg!(debug_assertions));
    }

    #[test]
    fn each_search_entry_names_its_own_page() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let index = search_index(locale);
            assert!(!index.is_empty());
            for entry in &index {
                assert_eq!(entry.page, title(entry.route, locale), "{entry:?}");
                assert!(!entry.header.is_empty());
                assert!(!entry.card.is_empty());
            }
        }
        // The Settings cards are findable now, in both languages.
        let en = search_index(Locale::EnUs);
        let found = search::find(&en, "appear");
        assert_eq!(found[0].route, Route::Settings);
        let zh = search_index(Locale::ZhTw);
        let found = search::find(&zh, "語言");
        assert_eq!(found[0].card, "language");
    }

    /// Updates, Health, and Activity are reachable from Apps and from the
    /// search, now that the pane does not list them.
    #[test]
    fn apps_opens_updates_health_and_activity() {
        use crate::model::Page;
        let links = apps_links(Locale::ZhTw, 2);
        let pages: Vec<Page> = links.iter().map(|link| link.page.clone()).collect();
        assert_eq!(pages, [Page::Updates, Page::Health, Page::Activity]);
        assert_eq!(links[0].header, "更新");
        assert_eq!(links[0].description, "2 個元件有新版本");
        assert_eq!(links[1].description, "檢查元件服務、相依套件與復原選項");
        assert_eq!(apps_links(Locale::EnUs, 0)[0].description, "Up to date");
        assert_eq!(
            apps_links(Locale::EnUs, 1)[0].description,
            "1 component has a new version"
        );
        assert_eq!(
            apps_links(Locale::EnUs, 3)[0].description,
            "3 components have new versions"
        );
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let index = search_index(locale);
            for link in apps_links(locale, 0) {
                let found = search::find(&index, link.header);
                assert!(
                    found
                        .iter()
                        .any(|entry| entry.card == link.card && entry.route == Route::Apps),
                    "{locale:?} search finds {}",
                    link.header
                );
            }
        }
        assert_eq!(
            search::find(&search_index(Locale::EnUs), "activ")[0].card,
            apps_card::ACTIVITY
        );
    }

    #[test]
    fn home_gathers_every_page_s_items_in_order() {
        let snapshot = Snapshot {
            components: vec![crate::attention::ComponentFacts {
                id: better_core::ComponentId::new("better-files").unwrap(),
                name: "Better Files".to_string(),
                status: manager_core::ComponentStatus::UpdateAvailable,
                drifted: false,
            }],
            packages_interrupted: true,
            ..Snapshot::empty(Locale::EnUs)
        };
        let items = attention(&snapshot);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].header, "A package installation was interrupted");
        assert_eq!(
            (module(Route::Apps).badge)(&snapshot),
            Some(NavBadge::Count(1))
        );
        assert_eq!(
            (module(Route::Apps).badge)(&Snapshot::empty(Locale::EnUs)),
            None
        );
        assert!(attention(&Snapshot::empty(Locale::ZhTw)).is_empty());
    }
}

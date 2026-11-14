//! Opening a page from outside the window, and keeping it one window (spec
//! section 1, "Opening a page from outside").
//!
//! `better-manager --page <page>` opens the window on that page. The window
//! owns `io.betteros.Manager` on the session bus and serves `ShowPage(s)`
//! there; a second `better-manager` finds the name taken, asks the running
//! window to show its page and raise itself, and exits 0, so there is never a
//! second window. The rules for the arguments and the strings that cross the
//! bus are here, apart from both the window and the bus, so they can be
//! tested; the bus code under them holds no rule of its own.

use better_core::ComponentId;

use crate::model::Page;
use crate::pages;

/// The session bus name the window owns. It is also the desktop entry's name
/// and the window's `app_id`.
pub(crate) const BUS_NAME: &str = "io.betteros.Manager";
pub(crate) const OBJECT_PATH: &str = "/io/betteros/Manager";
pub(crate) const INTERFACE: &str = "io.betteros.Manager";

/// Every name `--page` accepts, in the order `--help` lists them.
pub(crate) const PAGE_NAMES: [&str; 10] = [
    "home",
    "system",
    "touchpad",
    "keep-awake",
    "keep-awake/until",
    "apps",
    "updates",
    "default-apps",
    "fixes",
    "settings",
];

/// What the window is asked to show.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Request {
    /// A page. `until_dialog` is Keep awake's "Until a time" dialog, which
    /// the tray's "Until a time…" opens (ticket 87 draws the dialog).
    Page { page: Page, until_dialog: bool },
    /// A component's details in Apps, as `--component` asks for.
    Component(ComponentId),
    /// Nothing in particular: come to the front.
    Raise,
}

/// The page a `--page` name names.
pub(crate) fn parse_page(name: &str) -> Option<Request> {
    let page = |page| Request::Page {
        page,
        until_dialog: false,
    };
    match name {
        "updates" => Some(page(Page::Updates)),
        "keep-awake/until" => Some(Request::Page {
            page: Page::KeepAwake,
            until_dialog: true,
        }),
        other => pages::by_name(other).map(|module| page(module.route.landing())),
    }
}

/// The value of one valued option, written `--name value` or `--name=value`.
/// `None` when it is absent; an empty string when it was given nothing.
fn option_value(arguments: &[String], name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    arguments.iter().enumerate().find_map(|(at, argument)| {
        if argument == name {
            Some(arguments.get(at + 1).cloned().unwrap_or_default())
        } else {
            argument.strip_prefix(&prefix).map(str::to_string)
        }
    })
}

/// The refusal for a page that does not exist, in the two-language pattern
/// `better-manager` uses for an unknown argument.
pub(crate) fn unknown_page(name: &str) -> String {
    let list = PAGE_NAMES.join(", ");
    let list_zh = PAGE_NAMES.join("、");
    format!(
        "better-manager：沒有「{name}」這一頁。可以開啟的頁面：{list_zh}。\n\
         better-manager: there is no page \"{name}\". The pages are: {list}."
    )
}

/// What a command line asks the window to show, or why it is refused.
///
/// `--page` decides when it is given. Otherwise `--component` with a valid
/// id opens that component, and a malformed id is ignored as it always was:
/// the window still opens, on its usual page.
pub(crate) fn request(arguments: &[String]) -> Result<Option<Request>, String> {
    if let Some(name) = option_value(arguments, "--page") {
        return parse_page(&name)
            .map(Some)
            .ok_or_else(|| unknown_page(&name));
    }
    Ok(option_value(arguments, "--component")
        .and_then(|value| ComponentId::new(value).ok())
        .map(Request::Component))
}

/// How a request crosses the bus as `ShowPage`'s one string: a page name, a
/// component as `component:<id>`, or the empty string to only raise.
pub(crate) fn to_bus(request: &Request) -> String {
    match request {
        Request::Raise => String::new(),
        Request::Component(id) => format!("component:{id}"),
        Request::Page {
            until_dialog: true, ..
        } => "keep-awake/until".to_string(),
        Request::Page { page, .. } => match page {
            Page::Updates => "updates".to_string(),
            other => pages::module(crate::navigation::Route::of(other))
                .name
                .to_string(),
        },
    }
}

/// Reads `ShowPage`'s string. Anything this window would not accept on its
/// command line is refused here too, so the bus is no wider a door.
pub(crate) fn from_bus(value: &str) -> Option<Request> {
    if value.is_empty() {
        return Some(Request::Raise);
    }
    if let Some(id) = value.strip_prefix("component:") {
        return ComponentId::new(id).ok().map(Request::Component);
    }
    parse_page(value)
}

/// The session-bus half: owning the name, serving `ShowPage`, and calling
/// it on a window that already owns the name.
pub(crate) mod bus {
    use zbus::blocking::Connection;
    use zbus::fdo::{RequestNameFlags, RequestNameReply};

    use super::{BUS_NAME, INTERFACE, OBJECT_PATH, Request, from_bus, to_bus};

    struct ShowPage {
        sender: smol::channel::Sender<Request>,
    }

    #[zbus::interface(name = "io.betteros.Manager")]
    impl ShowPage {
        /// Shows a page and raises the window. A string the window would not
        /// accept as `--page` is refused with `InvalidArgs`.
        fn show_page(&self, page: String) -> zbus::fdo::Result<()> {
            let request = from_bus(&page)
                .ok_or_else(|| zbus::fdo::Error::InvalidArgs(super::unknown_page(&page)))?;
            self.sender
                .try_send(request)
                .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))
        }
    }

    /// The running window's hold on the name. Dropping it gives the name up.
    pub(crate) struct Owner {
        _connection: Connection,
        pub(crate) requests: smol::channel::Receiver<Request>,
    }

    /// What became of this process's claim to the name.
    pub(crate) enum Claim {
        /// This is the window; requests arrive on the receiver.
        Owner(Owner),
        /// Another window owns the name. Hand it the request.
        Running(Connection),
    }

    /// Connects to the session bus, or to `address` in a test.
    ///
    /// Every method call on the connection gives up after `timeout`: a
    /// window that owns the name and never answers, or something that is not
    /// Better Manager holding it, must not hang a second `better-manager`
    /// launched from a tray or a launcher.
    pub(crate) fn connect(
        address: Option<&str>,
        timeout: std::time::Duration,
    ) -> zbus::Result<Connection> {
        let builder = match address {
            None => zbus::blocking::connection::Builder::session()?,
            Some(address) => zbus::blocking::connection::Builder::address(address)?,
        };
        builder.method_timeout(timeout).build()
    }

    /// How long a second `better-manager` waits for the running window.
    pub(crate) const FORWARD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

    /// How long [`await_object_server`] waits for the object server in all.
    const OBJECT_SERVER_DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

    /// Returns once `connection`'s object server is reading method calls.
    ///
    /// zbus starts the server lazily: the first `object_server()` spawns a
    /// task that subscribes to incoming calls a moment later, and a call that
    /// reaches the connection before that is dropped with no reply (ticket
    /// 107). The server's root object answers `org.freedesktop.DBus.Peer.Ping`
    /// only once it is reading, so the connection pings its own unique name
    /// until it is answered. A ping sent too early is never answered; each
    /// attempt waits twice as long as the last, up to a quarter of a second, so
    /// a slow machine is not mistaken for a server that never started.
    fn await_object_server(connection: &Connection) -> zbus::Result<()> {
        let connection = connection.inner();
        let _ = connection.object_server();
        let Some(own_name) = connection.unique_name().map(ToString::to_string) else {
            return Ok(());
        };
        smol::block_on(async {
            let started = std::time::Instant::now();
            let mut attempt = std::time::Duration::from_millis(5);
            while started.elapsed() < OBJECT_SERVER_DEADLINE {
                let ping = async {
                    Some(
                        connection
                            .call_method(
                                Some(own_name.as_str()),
                                "/",
                                Some("org.freedesktop.DBus.Peer"),
                                "Ping",
                                &(),
                            )
                            .await,
                    )
                };
                let late = async {
                    smol::Timer::after(attempt).await;
                    None
                };
                match smol::future::or(ping, late).await {
                    Some(Ok(_)) => return Ok(()),
                    Some(Err(error)) => return Err(error),
                    None => attempt = (attempt * 2).min(std::time::Duration::from_millis(250)),
                }
            }
            Err(zbus::Error::Failure(
                "the D-Bus object server did not start answering calls".to_string(),
            ))
        })
    }

    /// Serves `ShowPage` and asks for the name. The object is served first,
    /// so a second window that finds the name taken always finds the method
    /// behind it.
    ///
    /// The object is added to a connection that is already built, because
    /// `ShowPage`'s channel is made here and [`connect`] hands out plain
    /// connections (a second window only forwards). So the name is asked for
    /// only after [`await_object_server`] has seen the server reading calls;
    /// a click forwarded the instant the name appears is then answered.
    pub(crate) fn claim(connection: Connection) -> zbus::Result<Claim> {
        let (sender, requests) = smol::channel::unbounded();
        connection
            .object_server()
            .at(OBJECT_PATH, ShowPage { sender })?;
        await_object_server(&connection)?;
        // DoNotQueue: a window that silently became the owner later, after
        // the first one closed, would be a second window that never opened.
        let owned = match connection
            .request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into())
        {
            Ok(reply) => matches!(
                reply,
                RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner
            ),
            Err(zbus::Error::NameTaken) => false,
            Err(error) => return Err(error),
        };
        if owned {
            Ok(Claim::Owner(Owner {
                _connection: connection,
                requests,
            }))
        } else {
            let _ = connection
                .object_server()
                .remove::<ShowPage, _>(OBJECT_PATH);
            Ok(Claim::Running(connection))
        }
    }

    /// Asks the window that owns the name to show `request`.
    pub(crate) fn forward(connection: &Connection, request: &Request) -> zbus::Result<()> {
        connection.call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some(INTERFACE),
            "ShowPage",
            &(to_bus(request),),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn page(page: Page) -> Request {
        Request::Page {
            page,
            until_dialog: false,
        }
    }

    #[test]
    fn every_spec_page_name_opens_its_page() {
        assert_eq!(parse_page("home"), Some(page(Page::Home)));
        assert_eq!(parse_page("system"), Some(page(Page::System)));
        assert_eq!(parse_page("touchpad"), Some(page(Page::Touchpad)));
        assert_eq!(parse_page("keep-awake"), Some(page(Page::KeepAwake)));
        assert_eq!(parse_page("apps"), Some(page(Page::Components)));
        assert_eq!(parse_page("updates"), Some(page(Page::Updates)));
        assert_eq!(parse_page("default-apps"), Some(page(Page::Defaults)));
        assert_eq!(parse_page("fixes"), Some(page(Page::Fixes)));
        assert_eq!(parse_page("settings"), Some(page(Page::Settings)));
        assert_eq!(
            parse_page("keep-awake/until"),
            Some(Request::Page {
                page: Page::KeepAwake,
                until_dialog: true
            })
        );
        for name in PAGE_NAMES {
            assert!(parse_page(name).is_some(), "{name}");
        }
    }

    #[test]
    fn a_page_name_is_exact() {
        for name in [
            "",
            "Home",
            "SYSTEM",
            "keep_awake",
            "awake",
            "apps/",
            "touchpad/gestures",
        ] {
            assert_eq!(parse_page(name), None, "{name}");
        }
    }

    #[test]
    fn the_command_line_names_one_page_in_either_spelling() {
        assert_eq!(request(&args(&[])), Ok(None));
        assert_eq!(
            request(&args(&["--page", "system"])),
            Ok(Some(page(Page::System)))
        );
        assert_eq!(
            request(&args(&["--page=keep-awake"])),
            Ok(Some(page(Page::KeepAwake)))
        );
        assert_eq!(
            request(&args(&["--component", "better-files"])),
            Ok(Some(Request::Component(
                ComponentId::new("better-files").unwrap()
            )))
        );
        // A malformed component id opens the window as before.
        assert_eq!(request(&args(&["--component", "../etc"])), Ok(None));
        // --page decides when both are given.
        assert_eq!(
            request(&args(&["--component", "better-files", "--page", "fixes"])),
            Ok(Some(page(Page::Fixes)))
        );
    }

    #[test]
    fn an_unknown_page_is_refused_in_both_languages() {
        for arguments in [args(&["--page", "bogus"]), args(&["--page=bogus"])] {
            let refusal = request(&arguments).expect_err("an unknown page is refused");
            assert!(refusal.contains("沒有「bogus」這一頁"), "{refusal}");
            assert!(refusal.contains("there is no page \"bogus\""), "{refusal}");
            for name in PAGE_NAMES {
                assert!(refusal.contains(name), "{refusal} lists {name}");
            }
        }
        let missing = request(&args(&["--page"])).expect_err("no page is refused");
        assert!(missing.contains("「」"), "{missing}");
    }

    #[test]
    fn a_request_crosses_the_bus_and_comes_back_the_same() {
        let requests = PAGE_NAMES
            .iter()
            .filter_map(|name| parse_page(name))
            .chain([
                Request::Raise,
                Request::Component(ComponentId::new("better-git").unwrap()),
            ]);
        for request in requests {
            assert_eq!(
                from_bus(&to_bus(&request)),
                Some(request.clone()),
                "{request:?}"
            );
        }
        assert_eq!(to_bus(&page(Page::Updates)), "updates");
        assert_eq!(to_bus(&page(Page::Components)), "apps");
        assert_eq!(to_bus(&Request::Raise), "");
    }

    #[test]
    fn the_bus_is_no_wider_a_door_than_the_command_line() {
        for value in ["bogus", "component:../etc", "component:", "Home", "/home"] {
            assert_eq!(from_bus(value), None, "{value}");
        }
    }
}

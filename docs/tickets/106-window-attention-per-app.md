# Ticket 106 — The person decides, per app, what happens when a window asks for attention

**Blocked by:** 84 (done) · **Status:** merged, not released

## Why

On the owner's Zorin 18.1 (GNOME Shell 46.0, Wayland), Claude Desktop
5.11492.2 asks for attention every time its window loses focus. GNOME turns
each request into a "「Claude」視窗已就緒" banner (`“%s” is ready`,
`js/ui/windowAttentionHandler.js` in `libshell-14.so`, connected to
`window-demands-attention` and `window-marked-urgent`), and the Zorin
taskbar marks the app's icon. The banner follows the app's notification
policy (`MessageTray.NotificationPolicy.newForApp`), so turning off the
app's banners in GNOME Settings hides it, but nothing controls the rest.

The owner reported the Claude Desktop behaviour to Anthropic on
2026-11-14 (feedback id `30a5398f-1a6b-49a6-9621-290a497b98ab`) and asked
for a Better Manager switch for this, set separately for each app, so any
app that does the same can be quieted.

## What to build

- **Four choices per app**, on a "Window attention" section of the control
  center's System page (ticket 86) or its own page if ticket 86 is not
  merged yet:
  - **Banner and taskbar mark** (GNOME's behaviour today).
  - **Taskbar mark only** (no banner, no entry in the notification list).
  - **Nothing** (the request is ignored).
  - **Bring the window forward** (the window is activated, as if the
    person had clicked it; for apps the person trusts).
- **A default for every app**, set to **Taskbar mark only**, with a list of
  apps that have their own choice. An app appears in the list after it has
  asked for attention once (Better OS records the app id and name), and can
  also be added by hand from the installed applications.
- **Done in Better OS's GNOME Shell extension** (within ADR 0012 and 0015):
  it replaces the handler on `global.display` for `window-demands-attention`
  and `window-marked-urgent`, looks up the window's app id
  (`Shell.WindowTracker`), and applies the choice. "Taskbar mark only" keeps
  the window's urgency so the taskbar still shows it, and clears it when the
  window is focused. The extension reads the choices from its GSettings
  schema; Better Manager writes them. Disabling the extension or
  uninstalling restores GNOME's handler exactly.
- Better Manager shows the setting only when the extension is enabled, and
  offers to enable it otherwise.

## Acceptance

- [x] In a nested or headless GNOME Shell 46 with the extension (ticket 102's
      headless method), a test window that demands attention produces, for
      each of the four choices, exactly the described result; screenshots.
- [x] A per-app choice overrides the default; removing it falls back.
- [x] Disabling the extension restores the stock banner.
- [x] Tests never change the person's real GNOME settings.
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      the extension's checks, and the license check pass.

## What was built

The owner chose, on 2026-11-14, an extension that only relays and a Rust
service that decides, rather than an extension that reads a settings schema;
[ADR 0018](../decisions/0018-shared-shell-adapter-extension.md) (proposed,
for the owner to accept) records it and widens the GJS exception to one more
directory. `AGENTS.md`'s language rule names both directories.

- **A shared Better OS GNOME Shell extension**, `shell-adapter@betteros.org` in
  `adapters/gnome-shell-adapter/`, packaged alone as `better-shell-adapter`
  (three files, no binary, no manifest). Its window attention relay serves
  `org.betteros.ShellAdapter1.WindowAttention`: the `WindowAttention(t, s, s,
  u, b, b)` signal and `SuppressBuiltInAttention(b)`, `ShowAttentionBanner(t)`,
  `ActivateWindow(t)`, `ClearAttention(t)`, `Capabilities()`. It acts only
  while `org.betteros.WindowAttention1` has an owner, takes GNOME's
  `WindowAttentionHandler` off `window-demands-attention` and
  `window-marked-urgent` only when asked, and puts it back exactly as GNOME
  connects it when that name goes and on `disable()`.
- **`window-attention-core`**: the four choices (`banner_and_mark`,
  `mark_only` as the default, `nothing`, `bring_forward`), per-app overrides,
  the decision, and the two files under
  `$XDG_CONFIG_HOME/better-os/window-attention/` with one writer each:
  `choices.json` (Better Manager) and `seen.json` (the service, every app that
  asked, capped at 512).
- **`window-attention-service`** (`/usr/libexec/better-window-attention`):
  owns `org.betteros.WindowAttention1`, asks the extension to take over
  whenever it appears, reads the choices for each request, records the app,
  and answers with one shell action. It ships in `better-manager` with a
  `Type=dbus` user unit and a session D-Bus activation file, because Better
  Manager enables and starts a component's units from its own record and
  `better-shell-adapter` has none. `better-manager` recommends (not depends
  on) `better-shell-adapter (= version)`, because its in-app update installs
  one verified `.deb` through the privileged service and the adapter is in no
  apt source; a Depends made that update fail. Its `prerm` stops and disables
  the service in every logged-in user's manager on removal; `install.sh` installs it when the release
  carries it, `build-deb.sh` builds it, `verify-deb.sh` checks both packages,
  CI lists it, and the container end-to-end check installs and removes it.
- **"Taskbar mark only" marks the taskbar through the Unity launcher API.**
  Zorin's taskbar draws its mark only from a `WindowAttentionSource`
  notification or `com.canonical.Unity.LauncherEntry.Update` with `urgent`
  (`zorin-taskbar@zorinos.com/notificationsMonitor.js`); it never reads a
  window's `demands-attention`, so keeping the request alone would have shown
  nothing. The service emits `Update("application://<id>", {urgent: true})`,
  keeps the request on the window, and the taskbar and mutter both clear their
  half when the app is focused.
- **Better Manager's Window attention page** (its own pane item, `--page
  window-attention`, registered in `PAGES`, until ticket 86's System page can
  hold it): the default as a list of the four choices with what the selected
  one does, an "Add an app" search over the installed applications, and every
  app with its own choice or that has asked, each with "Use the default (…)"
  and the four choices. Choosing "Use the default" removes the app's own
  choice. Until the extension is on, the page shows only a card that offers to
  turn it on (through `org.gnome.Shell.Extensions`, turning user extensions
  back on if the session has them off), or says why it cannot (installed after
  sign-in, not installed, failed, unsupported shell, no GNOME Shell). A service
  that does not answer is a warning with "Start it". An unreadable
  `choices.json` stops the page from writing and names the file. All of it is
  `better_ui::fluent`; the rules are in `window_attention_page`, without GPUI.

### Evidence

GNOME Shell 46.0, `gnome-shell --headless --wayland --no-x11
--virtual-monitor 1280x800` on a private `dbus-run-session`, with `HOME`, every
`XDG_*` directory, `XDG_RUNTIME_DIR`, and dconf in a temporary directory, the
extension linked into that home, Zorin's taskbar enabled, and the service from
this worktree started by D-Bus activation from a private service directory. A
GTK 4 test window (`io.betteros.AttentionTest`) asked for attention through its
own `present()` while another window covered it; mutter only sets a request on
an obscured window. The shell's state, the notification list, the banner, the
taskbar's own state, and a screenshot were read each time:

| Case | Window request | Banner, notification | Taskbar |
| --- | --- | --- | --- |
| No service (GNOME's own handler) | kept | shown | badge and mark |
| Banner and taskbar mark | kept | shown | badge and mark |
| Taskbar mark only | kept | none | marked (`urgent`), no badge |
| Nothing | cleared | none | none |
| Bring the window forward | window focused | none | none |
| Default banner, the app's own choice Nothing | cleared | none | none |
| The app's own choice removed | kept | shown | badge and mark |
| Extension disabled, service running | kept | shown (GNOME's) | badge and mark |
| Extension enabled again | kept | none | marked |
| Service stopped, extension enabled | kept | shown (GNOME's) | badge and mark |

Focusing the window cleared both the request and the mark. The page was
driven in the same session: choosing "Bring the window forward" for the test
app, then "Use the default", and adding an app from the search each wrote
`choices.json` and changed the next request accordingly; "Turn on" enabled a
disabled extension and the service took over; "Start it" started a stopped
service. Screenshots of the page in light and dark, English and Traditional
Chinese, are kept outside the repository. The person's own dconf
database still has none of the sandbox's values afterwards (no
`shell-adapter@betteros.org` in `enabled-extensions`, its own `idle-delay` and
`color-scheme`).

### After review

- Every extension method but `Capabilities` is served in the `…Async(params,
  invocation)` form and refuses (`AccessDenied`) any caller but the unique
  name the watcher reported for `org.betteros.WindowAttention1`; calls from
  another connection were refused in the sandbox and the takeover stayed on.
- The service believes a signal only from the extension's current unique name
  (`GetNameOwner` plus `NameOwnerChanged`); a forged `WindowAttention` sent
  straight to it is ignored, and the test fails without the check.
- The extension reports `WindowAttentionEnded(t, u)` when a reported window is
  focused, closed, or withdraws its request. The service remembers what it
  marked and sends `urgent: false` when the last marked window of an app ends,
  when Better Manager's `ChoicesChanged()` says the choice no longer marks it,
  and when the extension goes away. Seen on the bus in the sandbox: focus,
  close, and a choice changed from the page each produced `urgent=false`.
- `session-modes` includes `unlock-dialog`: with the screen locked, Nothing
  cleared the request, Banner made GNOME's notification, and Taskbar mark only
  marked, each as chosen.
- The bound test is an allow-list (top-level declarations, methods, fields,
  locals, conditions, comparisons, constructions), and a second test shows
  variants holding a list of quiet apps or remembering what asked fail it.
- The takeover retries with backoff for up to 30 s, and again whenever the
  extension reappears. Removing `better-manager` stops and disables the
  service in every logged-in session (`prerm`).
- The page's service card has the same layout as the other cards, every list
  is the same width, and each app shows its own icon (a letter when it has
  none), through a new `SettingsCard::leading` in `better_ui::fluent`.

### After the second review

- The extension accepts the owner of `org.betteros.WindowAttention1` only when
  `GetConnectionCredentials` names a process whose `/proc/<pid>/exe` is
  `/usr/libexec/better-window-attention`, and sends its two signals to that
  owner alone. ADR 0018 records what the check leaves open.
- `better-manager`'s window attention unit is always on: its postinst, built
  from `packaging/maintainer/user-units.sh`, enables it for every user
  (`systemctl --global`) and starts it for everyone logged in on install and
  on every upgrade; its prerm, from the same helper, stops and disables it for
  everyone on removal only. Before this, nothing started it until Better
  Manager was opened. `packaging/tests/maintainer-scripts.test.sh` covers both.
- On SIGTERM or SIGINT the service takes back every taskbar mark first.
- A companion can no longer also be a step or ride with two steps, and a
  manifest may not name another catalog component as a companion.
- A rollback of a package that APT never installed (the transaction failed
  before unpacking) counts as restored instead of trying `apt-get remove`.

### Better Manager's own update carries the adapter

A manifest artifact may name **companions** (`better_core::CompanionArtifact`):
first-party packages built from the same tree at the same version, which the
component's package recommends and no APT source carries. The plan carries
them (`PlanCompanion`), the driver fetches and stages each against its own
checksum, and the wire step carries them as `companions` (`WireCompanion`).
The privileged service checks each one like the step's own package (a Better
OS package, the cached bytes hash, the `.deb` names that package at the step's
version and the host's architecture), records a rollback point for each,
passes every `.deb` to one `apt-get install`, records each installed artifact,
and health-checks each; a failure in either fails the step and undoes both. A
plan with no companions is the same document protocol version 1 always was. A
privileged service from before this refuses the field before running anything;
the manager then sends the same plan without companions, so its own update
still works and only recommends the adapter. Tests:
`manager-ipc` (shape, naming, duplicates, limits, removal),
`better-core` (manifest validation), `manager-daemon` executor (one APT call
for both, both undone when the companion fails, a mislabelled companion refused
before APT), and `manager-core` exec (the companion travels in the step, and
the fallback for an older service).

### Not covered

- The test window's request came from a GTK 4 `present()` on Wayland; an X11
  urgency hint (`window-marked-urgent`) was not produced, since the session had
  no Xwayland. "Nothing" clears `demands-attention` only: an X11 client's own
  urgency hint is the client's to clear.
- Claude Desktop itself was not run in the sandbox.
- Nothing has been seen on the owner's real session. The extension takes
  effect after the package is installed and the person signs in again (GNOME
  on Wayland loads a newly installed extension at login).
- ADR 0018 is proposed and needs the owner's acceptance.
- The companion mechanism has not run against a real `apt-get` or a real
  release: `better-manager.yaml` can only name the adapter as a companion in
  the release that first publishes `better-shell-adapter`, with that release's
  checksums (`docs/release-packaging.md`). Until then an in-app update
  installs the manager alone and `install.sh` installs the adapter.


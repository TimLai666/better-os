# 46 — The manager says which version it is, and can be asked for updates

**Epic:** Field reports from the first real Zorin 18 install
**User Story:** A person running Better Manager can see which build they have
and ask it, on purpose, whether anything is out of date — and gets an answer
that says what was actually found.
**Blocked by:** 45
**Status:** done — branch `ticket-46`

## The two gaps

**Nothing showed the manager's own version.** Not the window, not the sidebar,
not a Settings page, and not the command line: `better-manager --version` was an
unknown argument, because the `clap` command declared a name and an `about` and
no `version`. A bug report could name the machine but not the build.

**There was no way to ask for an update check.** The catalog refreshed once at
launch and on the Components screen's "Update list" button, which reports on the
*list* and says nothing about updates. The Updates screen had neither the
button nor the freshness line, so "everything is up to date" was a sentence
about a catalog whose age the screen never showed.

## What was built

### The version, in three places

| Where | What it shows | Why there |
| --- | --- | --- |
| Settings → About | app name, version, this machine, project repository | the place a person looks for it |
| Sidebar header | `元件管理器 0.2.5` under the brand | already the line that names this application, and visible without navigating |
| `better-manager --version` | `better-manager 0.2.5` | what a bug report quotes |

The version is `env!("CARGO_PKG_VERSION")` in both binaries, so it is the
workspace version every crate inherits and there is no second number to keep in
step. `model::MANAGER_VERSION` is the GUI's single reading of it.

**The titlebar was left alone, deliberately.** `better_ui::window_chrome` is
shared chrome every Better OS window draws the same way; a version in Better
Manager's titlebar and nowhere else would be an inconsistency across the suite
rather than a feature, and the sidebar line beside it already names the
application.

The About platform line is the profile the manager actually planned from —
ticket 45's real probe, not a second reading of the host — so on the Zorin 18.1
machine it reads `zorin 24.04 · amd64`, and on a host the client could not
identify it reads `unknown unknown · unknown` rather than a guess.

The repository is shown as text rather than as a link. Nothing in this window
opens a browser, and a line that looked clickable and did nothing would be
worse than one that does not pretend.

### The update check

`ManagerApp::check_for_updates` sets one flag and calls `refresh_catalog` —
the *same* method the Components screen's button calls, which is the ticket-41
machinery: fetch on a background thread, validate every manifest as untrusted
input, cache the result, adopt whatever it decided. Two ways to ask the same
question that could disagree about the answer is the defect this avoids. The UI
thread is never blocked: the button says "正在檢查更新…" and is disabled while
the fetch runs, and the result appears when the refresh lands.

The result itself is a view model, `model::UpdateCheck`, with no GPUI in sight,
because what a check may claim is a decision and not a rendering detail:

| State | Shown | When |
| --- | --- | --- |
| `NotRun` | the section heading, nothing more | no check has been asked for |
| `Running` | 正在檢查更新… | the refresh is in flight |
| `Failed` | 這次檢查無法取得已發佈的清單, plus the degraded state's own sentence | the refresh fell back to the cache or to the built-in catalog |
| `Found` | 有 N 個元件可以更新 | the refresh landed and something is newer |
| `UpToDate` | 已是最新, plus how old the list is | the refresh landed and nothing is |

Two judgements are worth stating. A **failed** refresh never reports a count,
even though one exists: the number would describe the list the check failed to
replace, and it would read as this check's answer. A **partly refused** refresh
(some manifests rejected, the rest adopted) is *not* a failure — the newer
manifests are in force — so it reports the count, and the catalog line beside it
carries the refusal.

The Updates screen also grew the freshness row the Components screen has. It is
`catalog_status_row`, called from both pages, not a copy: one row, one
`CatalogLine`, one set of sentences.

## Tests

13 new, all hermetic, none reading the machine they run on.

- **`manager-gui`, 11** — the About section's version, platform, and repository
  against a Zorin 18 fixture host; an unidentifiable host reading `unknown`
  rather than 24.04; About copy present and translated in both locales; a check
  that found updates naming the count in both locales; a check that found
  nothing carrying the age, and carrying "never downloaded" when there is no
  fetch time; each of the three failure states reporting its own sentence and
  no count; a partly refused refresh still reporting its count; nothing claimed
  before a check is asked for and the running state outranking a previous
  result; the five new strings translated in both locales; the house overflow
  check over the new labels at three widths and three scales; and the
  repository URL fitting the narrowest supported window.
- **`manager-cli`, 2** — `--version` and `-V` against the shipped binary,
  printing the workspace version with no state file and no network.

## Verification

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo check --workspace` | clean |
| `cargo test --workspace` | 2,568 passed, 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| 9-second `ZED_HEADLESS=1` smoke, scratch XDG, `BETTER_MANAGER_OFFLINE=1` | window alive, no output |
| on-host run, real mode, scratch XDG | screenshots inspected, below |
| `better-manager --version` on the host | `better-manager 0.2.5` |

The on-host run was done in a nested sandbox compositor against a scratch XDG
tree, so nothing on the machine was touched. Started offline, the sidebar read
`元件管理器 0.2.5`, Settings → 關於 showed `版本 0.2.5`, `這台電腦 zorin 24.04 ·
amd64`, and the repository URL, and the Updates screen showed the check button
above the built-in catalog's "尚未下載過" warning. Pressing 檢查更新 fetched the
published manifests, and both rows changed together: 已是最新 · 剛剛更新, over
剛剛下載的清單 · 剛剛更新, with the outdated-list warning gone.

## Not done

- There is still no scheduled or background check. A refresh happens at launch,
  on either button, and on `better-manager catalog refresh` — a window left
  open for a week checks a week-old catalog until someone presses the button,
  and the age on screen is what says so.
- The repository line does not open. Making it clickable means a URL handler in
  the GUI, which is a decision about what this window may launch and not a
  presentation change.
- The `Failed` state's copy was exercised through the view model in both
  locales, not by making a real fetch fail on the host: the run above had a
  working network, and the offline start proves the never-refreshed state
  rather than a refusal mid-check.

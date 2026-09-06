# 44 — Field fixes: a component page that told the truth, and an update count that saw the machine

**Epic:** Field reports from the first real Zorin 18 install
**User Story:** The component page says what is actually installed, why the last
attempt failed, and what can be done about it; the Updates screen sees the
manager's own newer release; and an installed component can be removed.
**Blocked by:** 43
**Status:** done

## The machine this was written against

Verified with `dpkg-query` and the host's own `manager-state.json`:

| Fact | Value |
| --- | --- |
| `better-manager`, `better-manager-daemon` | installed by apt at `0.2.3` |
| `better-awake` | not installed |
| catalog version of every component | `0.2.4` |
| `manager-state.json` → `better-awake` | `installed_version: null`, `health: "failed"` |
| that failure's evidence | `daemon.error.plan_rejected:plan targets release 24.04 but this host is 18` |

The failure is from before ticket 43 fixed the daemon's release resolution, so
it describes a cause that no longer exists. What the window showed anyway:
`已安裝版本 0.2.4` for a component that was never installed, a red `異常` tag
with no reason on the page, `更新 0`, and no remove action anywhere.

## The four defects

### 1. The component page reported the catalog version as installed

`crates/manager-gui/src/pages_main.rs` drew the Overview tab's
`已安裝版本` row from `ComponentInfo::version_label()`, whose `None` arm
returned `available_version` — the catalog's version. So a component with
nothing installed reported the version it *would* get as the version it *has*.

Fixed by never folding the two into one row. `ComponentInfo::installed_label()`
returns the installed version or `未安裝`, and `可用版本` is its own row on both
the Overview and Versions tabs. `version_label()` survives only for the compact
card, where the not-installed case now reads `未安裝 · 0.2.4` rather than a bare
version sitting next to a red tag.

### 2. A stale failure rendered as an unexplained red tag

Three separate causes, all fixed:

- **The reason was never recorded.** The privileged service reports one string
  shaped `key:detail`. `outcome_to_stage` in `manager-core/src/exec.rs` stored
  it whole in `FailureRecord::evidence` and left `detail` empty, so ticket 43's
  Technical detail row — which reads `detail` — never appeared for the failure
  that motivated it. The daemon key is now split at the first `:`, and
  `FailureRecord::evidence_parts()` splits it at read time as well, so records
  already written on real machines become readable without being rewritten.
- **The failure was not on the page.** The failure card lived only in
  `restore_page`. It is now `ManagerApp::failure_card` in `components.rs`, shown
  by the recovery screen and by the component page: stage, localized evidence,
  and the service's own untranslated words.
- **There was no working retry.** A failure recorded before anything was applied
  leaves nothing installed, and every retry path offered `Verify`, which the
  planner refuses with `NotInstalled`. The detail page's action row had no
  action at all for that state. `ManagerApp::retry_operation` now picks `Install`
  when nothing is installed and `Verify` when something is, and the button says
  which.

Clearing on success was already correct and is now asserted: `Manager::finish`
sets `health` back to `Healthy` and drops `failure` for an install that
completes, so a retry after the cause is fixed leaves no trace of the old one.

### 3. Self-update was invisible

`Manager::status` computed from manager state alone. apt and the bootstrap
installer put Better OS on this machine, so state held nothing, every component
read `Available`, and `更新` counted zero while the catalog carried `0.2.4`.

`Manager::reconcile` — which already compared state against dpkg for drift —
now also **adopts** what only the host knows about.

**The adoption model.** A record is created with the version dpkg reports
(upstream form, so `0.2.3-1~ubuntu24.04` becomes `0.2.3`), `enabled: true`,
`health: Healthy`, and a new `ComponentRecord::provenance` field set to
`InstallProvenance::Dpkg`. It gets **no** `installed_artifact` and **no**
`restore_snapshot`, because none was ever captured, and the page says so:
*由系統套件管理員安裝，不是由 Better Manager 安裝，因此沒有可以還原的先前版本。*
Adopting is not drift and produces no `DriftFinding`: nothing was recorded, so
there is no disagreement. A version this crate cannot parse as a semantic
version is **left alone** rather than adopted, because every later comparison
would be made against a string nobody parsed — a package in that state stays
invisible, which is a known limit rather than a fiction. An update carried out
through the ordinary plan flow resets provenance to `Manager`.

The window runs this once at startup (`ManagerApp::reconcile_with_host`), which
is a read-only `dpkg-query` and therefore allowed outside the privileged
boundary. A demo window skips it: its state is a fabrication for screenshots and
mixing real host facts into it would make it neither.

**The self-update decision: through the daemon, with a restart notice.** Checked
against what the code actually does rather than assumed:

- `AptGetDriver::install_local_deb` runs `apt-get install ./file.deb` in a
  separate root process. Replacing `/usr/bin/better-manager` while the window is
  running is safe — the running process keeps the inode it already opened.
- The `better-manager` package declares `better-manager-daemon` as a
  **Recommends**, and the daemon installs with `--no-install-recommends`, so
  updating the manager does not pull a new daemon package. The service carrying
  out the transaction is never the package being replaced mid-transaction.
- `better-manager-daemon` is not a catalog component at all, so nothing claims
  to update it and nothing pretends to.

So no "run the installer yourself" fallback is needed. What is needed is saying
that the window on screen stays on the old version: `better-manager.yaml` now
declares `restart: application` instead of `none`, which flows through the
existing `RestartRequirement` machinery onto the card and the review screen, and
the review and finished screens carry an explicit notice keyed on the plan
touching `SELF_COMPONENT_ID` rather than on catalog contents.

### 4. No remove action

`DesiredOperation::Remove` was fully supported by `manager-core` and reachable
only from a card's overflow menu, never from the component page.

The component page now carries a `移除` action for anything installed, going
through the same plan → review → confirm flow as every other operation — the
review screen was already generic and needed no change.

**The self-removal decision: refused, with the reason on screen.** The catalog
has no "essential" concept and this ticket does not add one to the manifest
schema. Instead `manager-core` names the one component the manager is:
`SELF_COMPONENT_ID` / `is_self_component()`, and `resolve` refuses
`Remove` on it with `ManagerError::CannotRemoveSelf`. Refusing in core rather
than in a screen means the CLI cannot do it either. Removing the manager would
hand the privileged service a transaction whose own package is the one being
deleted, and leave the machine with no way to put any component back. The page
and the overflow menu omit the action and the page says why, rather than
offering a button that only produces a refusal.

## Two adjacent fixes the above required

- `pending_plan` was never cleared when a transaction ended, so every component
  in a finished plan kept offering "Review changes" and the finished screen had
  nothing to describe on the real path. A finished plan now moves to
  `finished_plan`, which the result screens read.
- `manager-cli reconcile` wrote state only when there were findings. Adoption
  changes state without producing one, so it now writes on a revision change and
  prints which components were installed outside Better Manager.

## Tests

24 added or rewritten.

- `manager-core/tests/lifecycle.rs` — `host_reconciliation` (8): the four
  combinations of recorded × on-host against a catalog that is newer and one
  that is level, decoration stripping, an existing failure surviving adoption,
  an unparseable host version being refused entry to the state file, and an
  adopted component updating through the ordinary flow.
- `manager-core/tests/lifecycle.rs` — `self_component` (2) and
  `failure_clearing` (2): self-removal refused while every other component still
  removes, the manager updating through the same plan, a successful install
  clearing the previous attempt's failure and health, and a pre-apply failure
  leaving nothing to verify.
- `manager-gui/src/tests.rs` — `component_page` (8): the installed and available
  rows for not-installed, manager-installed, and apt-installed components; the
  field's own evidence string splitting into a key and the reason; the failure
  reaching the page whole; the restart notice firing only for the manager's own
  plan; self-removal refused with copy in both locales; every new string present,
  translated, and laying out no worse than the labels already shipped.
- One existing test (`a_component_that_was_never_installed_is_not_drift`) was
  replaced rather than weakened: its subject — a package only the host knows
  about — is the thing this ticket deliberately changed.

## Verification

- `cargo fmt --all`, `cargo check --workspace`, `cargo test --workspace`
  (167 suites green), `cargo clippy --workspace --all-targets -D warnings`.
- Two 8-second headless runs (`ZED_HEADLESS=1`, `BETTER_MANAGER_OFFLINE=1`,
  scratch `XDG_STATE_HOME`/`DATA`/`CONFIG`/`CACHE`), one over a copy of the field
  state file and one in demo mode. Zero stderr from both. The real-mode run
  adopted `better-manager 0.2.3` from dpkg and left `better-awake`'s failure
  record untouched; the demo run wrote nothing, as it should.
- An on-compositor run against a copy of the field state, screenshotted at each
  screen: Overview reads 已安裝 1 / 可更新 1 (was 0); the Better Awake page reads
  `已安裝版本 未安裝`, `可用版本 0.2.4`, a `上次安裝失敗` card with
  `失敗階段 安裝檔案`, `佐證 系統服務認為這個計畫與這台機器不符，已拒絕執行`,
  `技術細節 plan targets release 24.04 but this host is 18`, and a `重新安裝`
  button; the Better Manager card reads `0.2.3 → 0.2.4 · 有新版可用 ·
  需重新啟動應用程式`; its page shows the provenance sentence, offers
  更新 / 停用 / 重新檢查 and no 移除, and states why.

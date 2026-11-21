# ADR 0045: Better OS moves to GPUI Kit's current release, on GPUI Fast

## Status

Accepted on 2026-11-21 for the two things the owner decided that day
(ticket 257): GPUI Kit is upgraded to its latest release, and the GPUI under
it is GPUI Fast. Nothing measured for this record rules GPUI Fast out. Open
questions for the owner are listed at the end; none of them blocks the
upgrade branch.

This record replaces ADR 0020's choice of repository sources for GPUI and
the component library with crates.io, and says when ADR 0043's fork stops
being needed. Until the upgrade branch lands, ADR 0020 and ADR 0043 describe
what the workspace builds.

## Context

Every Better OS window is built on `gpui-component`, pinned by git revision
(`longbridge/gpui-component` `88f102d1`, 2026-07-30), on GPUI taken from
Zed's repository at `ae394f3` (2026-07-30) through the owner's fork, which
adds one Wayland fix (ADR 0043). Since then:

- The component library is **GPUI Kit** (`longbridge/gpui-kit`), 0.10.0 on
  crates.io (2026-11-16), 930 commits after the pinned revision. It is split
  into `gpui-base` (behaviour, state, `Root`) and `gpui-component` (the
  styled components); `gpui-kit` is a facade over both, and
  `gpui-kit-assets` is the icon set. It takes GPUI from `gpui-pre-*`, weekly
  unmodified snapshots of Zed's crates that Longbridge publishes (0.10.0
  pins `gpui-pre =0.3.12`, Zed `d2a4fb6`, 2026-11-08), and builds on
  **GPUI Fast** instead when its `gpui-fast` feature is on.
- **GPUI Fast** (`longbridge/gpui-fast`, 0.1.17, 2026-11-17) is Zed's GPUI
  with retained views, scroll layers and native window composition kept in
  `fast/` modules over upstream files (its `docs/upstream-sync.md`). It is
  synced from Zed weekly and calls itself experimental.
- The Wayland fix of ADR 0043 was offered to GPUI Fast as
  longbridge/gpui-fast#173 on 2026-11-21.

## Decision

1. **GPUI Kit 0.10 (or the latest release when the upgrade branch starts)
   from crates.io**, with its `gpui-fast` feature on, replaces the
   git-pinned `gpui-component`. The workspace depends on `gpui-component`,
   `gpui-base` and `gpui-kit-assets` directly, under the keys the code
   already uses (`gpui-component-assets = { package = "gpui-kit-assets", … }`),
   rather than on the `gpui-kit` facade, so no `use` line changes.
2. **GPUI Fast from crates.io** is the workspace's `gpui` and
   `gpui-platform` (`gpui = { package = "gpui-fast", … }`,
   `gpui-platform = { package = "gpui-fast-platform", features =
   ["wayland", "x11"] }`), at a version the chosen GPUI Kit release
   accepts. Every version is exact (`=`) and moves only in a change that
   says so, as GPUI Kit's own pins are.
3. **ADR 0043's fork while GPUI Fast lacks the fix.** If no GPUI Fast release
   carries #173 when the upgrade branch lands, the fix is carried (unless the
   owner chooses to wait, under "Open questions") as a
   `[patch.crates-io]` of `gpui-fast-linux` from the owner's fork, under ADR
   0043's rules (one commit, offered upstream, tagged, never moved), and the
   section is removed in the change that moves to the first release that has
   it. The `[patch."https://github.com/zed-industries/zed"]` section and the
   Zed git dependency go in the upgrade branch either way.
4. **GPUI on `gpui-pre` stays the way back**, not a second build: if GPUI
   Fast stops being maintained or a defect in it has no fix in reach, the
   workspace turns the `gpui-fast` feature off and names `gpui-pre`, with
   the same `[patch.crates-io]` mechanism for any fix it needs (below,
   "The way back").

## The two GPUI choices under GPUI Kit 0.10

Both were measured or read for this record on 2026-11-21. "Today" is the
workspace at `339e12e`.

| | `gpui-pre` 0.3.12 (Zed's GPUI) | GPUI Fast 0.1.17 |
| --- | --- | --- |
| Upstream base | Zed `d2a4fb6` (2026-11-08), 202 commits behind Zed's `main` on 2026-11-21 | Zed `78f5d74` (2026-11-12), 145 behind |
| Viewport fix of ADR 0043 | absent (`gpui-pre-linux` `src/linux/wayland/window.rs` line 1546 sends the size unconditionally); Zed's own fix, zed-industries/zed#61368, open since 2026-07-20 | absent in the release (`gpui-fast-linux` `window.rs` line 1574; reproduced on sway, below); #173 open, approved by its maintainer on 2026-11-21, not merged; its diff applies to the published crate and fixes it |
| Rounded clipping (ticket 141's limit) | none: `ContentMask` is a rectangle | `.rounded(r).overflow_hidden()` clips children to the curve when drawn (`fast/rounded_clip.rs`, wgpu shader in `gpui_wgpu/src/fast/rounded_clip.rs`, since gpui-fast#76, 2026-10-17); `ContentMask` is still a rectangle and hit testing is not rounded |
| Layer-shell API | identical to the pinned revision (`platform/layer_shell.rs`, `diff` empty) | identical |
| Outputs that go away | removed from the list (`client.rs` `GlobalRemove`, line 1640), `App::observe_displays` | the same (line 1643) |
| Retained views | none | on; a render that reads state outside entities, globals, list or scroll state must `cx.notify()`, or it shows stale content (`docs/retained-mode.md`); `GPUI_VIEW_RETENTION=0` turns retention off |
| What it builds | one GPUI | GPUI Fast **and** `gpui-pre`'s core: GPUI Kit 0.10 lists `gpui-pre` as a non-optional dependency, so it compiles unused |
| Cost to Better OS code | the same edits as GPUI Fast, less the Zed changes made after `d2a4fb6` (they arrive with a later snapshot) | measured below |

Every GPUI API change the scratch build met came from Zed between `ae394f3`
and `78f5d74`, not from GPUI Fast: `gpui-fast` 0.1.17's `WindowBounds::centered`
and its blanket `impl<T: Element> IntoElement for T` are Zed's at `78f5d74`
(`crates/gpui/src/platform.rs` line 2827, `element.rs` line 145), and
`gpui-pre` 0.3.12 will meet them at its next snapshot. GPUI Fast's rule is no
public API of its own beyond the exceptions its `docs/upstream-sync.md`
lists (dynamic textures, window composition, scene layers), none of which
Better OS uses.

## What the move costs: the API distance

Measured by switching the workspace to GPUI Kit 0.10 on GPUI Fast 0.1.17 and
fixing only what stopped the build (branch
`scratch/257-gpui-kit-0.10-on-gpui-fast`, commit `f908c2c`).
`cargo check --workspace --all-targets --keep-going` then builds every crate
except the Git window. rustc reports borrow-check errors only after a crate
type-checks, so with everything else checking, nothing is hidden behind the
errors counted here.

| Crate | Compiler errors | What it took | Size |
| --- | --- | --- | --- |
| `better-ui` | 5 (lib), 2 more in tests | `ScrollbarShow`/`scrollbar_show` → `ScrollbarMode`/`mode`; `Root::bordered(false)` removed (gpui-base's `Root` draws no border); two `impl IntoElement` removed (blanket impl); tests: `ScrollEvent.source`, `Keystroke::parse` takes `&'static str` | small |
| `app-chooser-gui`, `awake-gui`, `files-gui`, `fluent-gallery`, `launcher-gui`, `manager-gui`, `monitor-gui`, `touchpad-gui` | 1 each (launcher-gui 1 more in tests) | `WindowBounds::centered(None, …)`, 10 call sites; launcher test `&'static str` | small |
| `layer-check` | 3 | `Keystroke::unparse(OperatingSystem::Linux)` | small |
| `git-gui` | 1 (more expected) | `InputState::multi_line` is gone; a multi-line field is `TextareaState` with `Textarea`, so `description`, `co_authors` and the third dialog field change type, constructor, element and the code that reads them | medium |

Nothing Better OS uses was removed without a replacement. Behaviour the
compiler cannot see, which the upgrade branch must look at window by window:
retained views (above); `Root` no longer drawing a border; 930 commits of
component behaviour (scroll bounce now on by default only on macOS, input,
tooltips, focus, accessibility); the icon files of the Lucide set behind
`IconName`; theme defaults.

- **Themes and icons.** `Theme`, `ThemeMode`, `ThemeTokens`, `ActiveTheme`,
  `Icon`, `IconName` (every variant Better OS names) and `TitleBar` all
  compile unchanged.
- **Features.** `gpui-fast-platform` has `wayland` and `x11`; `gpui`'s
  `layer_shell`, popups, input regions and exclusive zones build unchanged
  (`better_ui::layer` and `layer-check` compile); the client-side frame of
  `better_ui::system_frame` compiles unchanged; text input comes from
  `gpui-component`'s `Input`; no Better OS crate calls GPUI's accessibility
  API directly.
- **The assets crate and an HTTP client.** Neither the pinned
  `gpui-component-assets` nor `gpui-kit-assets` 0.10 links one into a Linux
  binary: both declare their `reqwest` for `wasm` only
  (`crates/assets/Cargo.toml`), and `cargo tree -e normal --target
  x86_64-unknown-linux-gnu -i zed-reqwest` (today) and `-i gpui-pre-reqwest`
  (scratch) print nothing for `app-chooser-gui`, `better-ui`, `files-gui` and
  `manager-gui`; `-i hyper` prints nothing in either. AGENTS.md's note that
  every desktop binary links one, and the comment in
  `crates/launcher-gui/tests/dependencies.rs`, come from that test's
  lockfile walk, which counts every target's dependencies.
- **Toolchain.** The scratch `better-ui` checks with Rust 1.95, the
  workspace's `rust-version`, and with 1.98.1, GPUI Fast's own toolchain.

## Measurements

All on the owner's laptop (AMD Ryzen AI 9 HX 370, 24 threads, Radeon 890M,
Zorin OS 18, Rust 1.100.0), on 2026-11-21, with other
builds sharing the machine (load average 1 to 10 during the runs). Logs are in
`/home/tim/Developer/better-os-evidence/257/logs/`.

**Rendering CPU: GPUI Fast's own showcase, not Better OS.** No Better OS
window was drawn on GPUI Fast for this record, so these are `gpui_perf`'s
numbers (GPUI Fast `v0.1.17`), built twice in release as its README says:
on GPUI Fast (`cargo build -p gpui_perf --release`) and on upstream GPUI
(`--no-default-features --features upstream`, which is `gpui-pre` 0.3.7,
the snapshot that showcase pins, older than GPUI Kit 0.10's 0.3.12). Each ran
`--auto` twice, alternating, in a headless sway 1.9 (one 1600×1000 output,
scale 1, 60 Hz) under the sandbox rules. Which Vulkan device wgpu chose was
not logged (the loader mapped every installed driver). Main-thread CPU per
frame, median, mean of the two runs; process CPU in parentheses (100 % is one
core); GPUI Fast with retained views on:

| Scenario | `gpui-pre` 0.3.7 | GPUI Fast 0.1.17 |
| --- | --- | --- |
| A spinner animating, nothing else changing | 11.93 ms (77 %) | 2.65 ms (16 %) |
| Scrolling the sidebar | 12.54 ms (80 %) | 2.47 ms (17 %) |
| Scrolling a page of components | 11.99 ms (75 %) | 0.92 ms (9 %) |
| Scrolling the data table | 7.40 ms (47 %) | 2.23 ms (16 %) |
| Refreshing the table every 33 ms | (27 %) | (10 %) |
| Scrolling the list | 6.21 ms (40 %) | 1.40 ms (11 %) |
| Quotes streaming into the workspace | 16.0 ms (67 %), 32 fps | 7.96 ms (44 %), 55 fps |
| Scrolling the workspace's watchlist | 15.4 ms (95 %), 54 fps | 8.83 ms (54 %), 62 fps |
| Hovering the watchlist | 15.7 ms (94 %), 52 fps | 8.93 ms (53 %), 62 fps |

With retained views off, GPUI Fast still took 3.3 to 4.1 ms in the first
six rows. Memory: the showcase's peak resident set was 146 and 151 MB on
`gpui-pre` and 184 and 187 MB on GPUI Fast (its scroll-layer tiles and
retained trees). The showcase has no "nothing at all changes" scenario, so
idle CPU while drawing is not in this table.

**Better OS's own harnesses**, `cargo bench -p launcher-gui --bench
launcher_suite` and `-p files-gui --bench files_suite`, run once each on
today's workspace and on the scratch branch. Both are headless
(`ZED_HEADLESS=1`, no compositor, no frame), so they measure process start,
models and idle, not drawing:

| Launcher | today | scratch (GPUI Fast) |
| --- | --- | --- |
| spawn to first renderable model, p50 | 525 ms | 472 ms |
| `main()` to focused search row, p50 | 70.5 ms | 65.1 ms |
| CPU over a 20 s idle window | 0.0000 % | 0.0000 % |
| resident set after idle | 103 028 kB | 113 208 kB |

The launcher's GPUI-free index build moved from 255 to 149 ms between the
two runs, which is the size of this machine's noise; only the 10 MB of
resident memory is a difference beyond it. `files_suite` measures no GPUI
code (listing, search, previews, copies) and its rows differ within the same
noise. Monitor has no benchmark harness.

**Build cost** (finding 4): a clean `cargo check` from a fresh `CARGO_HOME`,
fetched first, timed inside the build slot (`CARGO_BUILD_JOBS=6`,
`CARGO_INCREMENTAL=0`, no debug info), two runs each:

| | today (Zed fork + git `gpui-component`) | scratch (crates.io, GPUI Fast) |
| --- | --- | --- |
| `cargo check -p better-ui` | 48.9 s, 51.0 s | 49.9 s, 56.2 s |
| `cargo check -p app-chooser-gui` | 58.3 s, 59.8 s | 72.6 s, 71.1 s |
| `target/` after `better-ui` / `app-chooser-gui` | 647 MB / 930 MB | 707 MB / 934 MB |
| `CARGO_HOME` after `cargo fetch --locked` | `git/` 517 MB (Zed fork: 371 MB repository, 91 MB checkout; `gpui-component` 19 + 12 MB), `registry/` 1.4 GB | no `git/`, `registry/` 1.6 GB (the GPUI families: 53 MB) |
| `cargo fetch --locked` (network, one run) | 300 s | 27 s |

The window checks 20 % slower on GPUI Fast because it compiles two GPUI
cores (above). What crates.io saves is the Zed checkout: half a gigabyte per
`CARGO_HOME`, and a fetch that does not clone Zed. The owner's `~/.cargo`
holds two Zed repositories (upstream and fork, 363 and 371 MB) and their
checkouts today.

**Layer-shell surfaces on GPUI Fast at run time.** `better-layer-check`
built on the scratch branch, in `packaging/tests/session-layer-facts.sh
outputs` (headless sway, two outputs, one added, one removed, one added):

- at scale 1, a full-width strip and its popup followed every output, and
  the removed output's strip closed;
- at scale 2, a fixed-size surface did the same;
- at scale 2, the **full-width strip was disconnected** with `Protocol error
  0 on object wp_viewport@27` before it was drawn: ADR 0043's defect, on
  GPUI Fast 0.1.17;
- the same with #173's diff applied to `gpui-fast-linux` 0.1.17 and given to
  the workspace as `[patch.crates-io] gpui-fast-linux = { path = … }`: no
  protocol error, both strips at 1280×32 at scale 2, the popup, the output
  added and removed, all as at scale 1.

Screenshots and logs are in
`/home/tim/Developer/better-os-evidence/257/layer-outputs-gpui-fast/` and
`…/layer-outputs-gpui-fast-with-173/`. `cosmic-comp` was not run.

## The plan for the upgrade branch

One branch, started after the open pull requests have landed, in this order:

1. **Screenshots before**: every window, light and dark, zh-TW and English,
   from `main` before the branch, the way ticket 257 asks, saved outside the
   repository.
2. **The workspace manifest**: Decision 1 and 2, the Zed git dependency and
   its patch section removed, the viewport fix as in Decision 3, the
   lockfile, `docs/third-party-licenses.md` regenerated in the same commit,
   and `rust-version` confirmed by the CI `msrv` job.
3. **`better-ui`** first, because every window builds on it: the edits above,
   then its tests. Its layer-shell surfaces are checked again with
   `packaging/tests/session-layer-facts.sh` on `cosmic-comp` at 150 % and
   200 % (the viewport fix) and the `outputs` case on sway.
4. **The windows**, smallest first: `app-chooser-gui`, `fluent-gallery`,
   `touchpad-gui`, `awake-gui`, `launcher-gui`, `monitor-gui`, `files-gui`,
   `manager-gui`, `git-gui` (the text areas), then `layer-check`. For each, a
   search of its render code for state read outside entities (a shared
   `Rc<RefCell<…>>`, a clock, a channel polled in `render`) and a run with
   `GPUI_VIEW_RETENTION=0` against the default when anything looks stale.
5. **The licence review** (`engineering/specs/copyleft-review.md`): the Zed
   crates are all Apache-2.0 now (below), its Decision 4 is settled by the
   snapshots' own fields, and the Corresponding Source section is rewritten
   for crates.io sources and, while it exists, the patched `gpui-fast-linux`.
6. **Packaging and CI**: the cargo cache keys that name the Zed checkout,
   `packaging/build-deb.sh` for all four targets (Ubuntu 22.04 and 24.04,
   amd64 and arm64), `verify-deb.sh`.
7. **Proof**: the workspace gate (fmt, clippy on the newest stable, every
   test), CI green on the four package targets, `cargo bench -p files-gui
   --bench files_suite` and `-p launcher-gui --bench launcher_suite` against
   the numbers here, and screenshots after, compared one by one with the
   ones before, so no window lost anything.
8. **Records**: this ADR's "Unverified" list worked through, ADR 0020 and
   ADR 0043 marked as replaced where they no longer describe the build,
   `engineering/architecture.md`'s GPUI paragraph, and AGENTS.md's
   follow-ups that name the toolkit (the HTTP client note, ticket 141's
   limit if rounded clipping is then used).

What is kept from ADR 0043: its rule for what a fork may carry and its
Corresponding Source discipline, for as long as Decision 3 needs a patch.
What is dropped: the Zed fork's branch as a build input (its tags stay,
because published packages were built from them).

Using GPUI Fast's rounded clipping to lift ticket 141's recorded limit is
not part of the upgrade; it is a change of `better_ui::fluent::window_frame`
and its own ticket after it.

## Licences

All 37 crates of the three families the scratch build resolves
(`gpui-kit*`, `gpui-base`, `gpui-component*`, `gpui-fast*`, `gpui-pre*`)
declare `Apache-2.0` on crates.io, `gpui-pre-reqwest` `MIT OR Apache-2.0`
(crates.io API, read 2026-11-21).

- **The two unlabelled Zed crates.** `gpui-pre-shared-string` and
  `gpui-pre-util` 0.3.12 declare Apache-2.0, as their Zed sources do at
  `d2a4fb6`: the fields were added upstream after `ae394f3`.
- **No GPL crate from Zed is left.** `zlog`, `ztracing` and
  `ztracing_macro`, GPL-3.0-or-later at `ae394f3`, were relicensed by Zed
  under Apache-2.0 on 2026-09-01 (zed-industries/zed#63573), and
  `gpui-pre-zlog`, `-ztracing` and `-ztracing-macro` 0.3.12 say so.
- **Corresponding Source.** The windows are still conveyed under
  GPL-3.0-or-later, so their Corresponding Source still includes every
  dependency's source. A crate built unmodified from crates.io is that
  source as published: crates.io keeps every published version downloadable
  (a yanked version too), so the offer can point at it with the versions
  `Cargo.lock` names, and Better OS no longer has a modified file of its own
  in the graph. GPUI Fast's own modified Zed files already carry Longbridge's
  modification notices (Apache-2.0 section 4(b)). While Decision 3's patch
  exists, the patched `gpui-fast-linux` is Better OS's modification again,
  with ADR 0043's obligations: a tagged fork revision that never moves, and
  a notice in the changed file.

## The way back

GPUI Fast calls itself experimental. What was found on 2026-11-21:

- **Cadence.** 18 releases from v0.1.0 (2026-10-06) to v0.1.17
  (2026-11-17); a Zed sync roughly weekly (six `Sync GPUI upstream`
  commits, 2026-10-06 to 2026-11-09, and `78f5d74` in v0.1.16). Breaking
  changes in the release notes: 0.1.11 renamed `ScrollWheelEvent` to
  `ScrollEvent` (old name kept as a deprecated alias), and 0.1.16 took Zed's
  blanket `IntoElement`. Since the import of `78f5d74`, Zed changed
  `crates/gpui` in 13 commits and `crates/gpui_linux` in 3.
- **People.** The `fast/` code, docs and showcase are mostly Jason Lee
  (huacnlee, 42 commits) and Sunli (30); huacnlee is the only crates.io owner
  of `gpui-fast` and of `gpui-pre`. Of the last 30 pull requests
  (2026-11-15 to 11-21), 19 were merged, with a median of 16 hours from
  opening, 4 closed and 7 open.
- **GPUI Kit's CI** runs a `GPUI Fast` job on Linux, macOS and Windows and
  one for WebAssembly on every push (`cargo test -p gpui-kit --features
  gpui-fast,test-support`, `-p gpui-component --features
  gpui-fast,test-support --lib`); the latest run on `main` (2026-11-20) was
  green in all of them. Whether that job is required for merging could not
  be read (branch protection is not public).
- **The way back is a feature and a pin.** GPUI Kit builds on `gpui-pre` with
  the feature off, and its `gpui-pre` crates are already in the build. A fix
  either GPUI needs is carried as `[patch.crates-io]` with a package of the
  same name and version: tried with `gpui-kit =0.10.0` and modified copies
  of `gpui-pre-linux` 0.3.12 and `gpui-fast-linux` 0.1.17, and `cargo tree`
  resolved both to the copies with no "patch not used" warning (as a path;
  a git source uses the same rule, not tried). ADR 0043's Zed fork cannot be
  that source as it is: its crates are named `gpui_linux`, not
  `gpui-pre-linux`, so the fork would carry the published crate's source
  under its published name, the way GPUI Fast's own release step
  (`docs/releasing.md`) generates it. GPUI Fast's `compat/` crates, which
  patch `gpui-pre` to GPUI Fast, are at `=0.3.9` and do not apply to GPUI
  Kit 0.10; the `gpui-fast` feature is the supported route.

## Unverified

- No Better OS window has been opened on GPUI Fast or on GPUI Kit 0.10: the
  scratch build checks and runs headless benchmarks only.
- The viewport defect and #173's fix on GPUI Fast were seen on sway only,
  not on `cosmic-comp`; the patch was a path, not the fork's git revision.
- Rendering CPU of a Better OS window on GPUI Fast: the numbers above are
  the showcase's, against an older `gpui-pre` than GPUI Kit 0.10's.
- The Git window on GPUI Kit 0.10.
- The four package targets, and Ubuntu 22.04's libraries under GPUI Fast.
- Whether GPUI Kit's `GPUI Fast` CI job is required before a merge.
- Every behaviour change the compiler cannot see (above), including
  retained-view staleness in any window.

## Open questions for the owner

Nothing found rules GPUI Fast out. Two choices are the owner's:

- Carry #173 as a `[patch.crates-io]` of `gpui-fast-linux` from the owner's
  fork if no GPUI Fast release has it when the upgrade branch lands
  (Decision 3), or hold the branch until a release has it. Recommended:
  carry it; the patch was shown to work above and goes with the next pin
  move. The fork would be a new repository or branch holding the published
  crate under its published name, which ADR 0043's rule allows but its
  branch naming does not describe.
- Accept the costs measured here: about 20 % longer clean checks of a window
  (two GPUI cores until GPUI Kit makes `gpui-pre` optional), about 10 to
  40 MB more resident memory per window, and one person who owns every GPUI
  crate on crates.io the workspace would use.

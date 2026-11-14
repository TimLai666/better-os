# 41 — Remote catalog refresh

**Epic:** Distribution: Better Manager installs the current release's
components without waiting for its own next build.
**User Story:** Better Manager fetches the published component manifests,
verifies them, and offers every component of the latest release — so the
built-in catalog's release-lag (a 0.2.1 manager ships 0.2.1-placeholder
manifests) stops mattering.
**Blocked by:** none (40 is independent)

## Problem being solved

Manifest checksums can only be written after their release is published, so
the binaries of release N always embed a catalog that cannot verify release
N's artifacts. Recorded as a structural follow-up since v0.2.0. The fix is a
refreshable catalog: the source of truth moves to the repository's `main`
branch (whose manifests carry the published checksums of the latest release),
fetched over HTTPS and validated as untrusted input.

## What it delivers

- A catalog-source seam in manager-core/platform: `built-in` (embedded, the
  offline fallback) and `remote` (fetched). An ADR records the decision: what
  is fetched (the seven manifest files, or a single generated
  `catalog.json` bundle — choose and justify), from where (raw.githubusercontent
  main, pinned to the repo), the trust model (HTTPS + full manifest
  validation + artifact checksums; signing stays deferred and the ADR says
  what changes when it lands), refresh cadence (on demand + on manager start
  with a bounded timeout, never blocking the UI), and failure behavior
  (stale-but-honest: show the built-in catalog with a visible "catalog may be
  outdated" state, never silently).
- Remote manifests are untrusted: full existing validation applies; a
  manifest that fails validation is rejected individually with the reason
  surfaced, and never replaces a valid cached one.
- Cached refreshed catalog on disk (versioned, in the manager's state dir)
  so offline restarts keep the last good catalog; cache records fetch time
  and source.
- A downgrade guard: a fetched manifest with a lower version than the cached
  one for the same component is flagged, not silently adopted (protects
  against stale CDN/rollback confusion).
- CLI: `better-manager catalog refresh|status` equivalents sharing the same
  core logic; GUI: a refresh affordance and a last-updated/degraded state on
  the Components screen (keep it small — a status line and a button, not a
  new page).
- Tests: fetch-and-validate against fixture servers (local HTTP or injected
  fetcher trait — follow manager-platform's artifact-download seam), rejection
  suite (invalid manifest, wrong schema, downgrade, partial fetch), cache
  round-trip and offline fallback, UI state mapping, CLI output. No network
  in the default test suite.
- End-to-end proof (container or local with the real network, `#[ignore]`d):
  a manager with the 0.2.1-placeholder built-in catalog refreshes and can
  then plan an install of a real component with verified checksums.

## Out of scope

- A public APT repository; package signing (deferred decisions unchanged).
- Auto-install or auto-update of components (refresh updates the catalog,
  not the host).

## Verification

Workspace gates; the fixture-server suite; the ignored real-network proof
run once and reported honestly; headless GUI smoke.

## What was built

`manager-platform::catalog_fetch` is the seam: a `ManifestFetcher` trait,
`HttpManifestFetcher` (HTTPS only, ten-second global timeout, 256 KiB per file,
base pinned to `raw.githubusercontent.com/TimLai666/better-os/main`, overridable
with `BETTER_MANAGER_CATALOG_URL`), and `StaticManifestFetcher`, which answers
from supplied documents so nothing above it needs a network. It parses no YAML,
in the same way the artifact downloader next door verifies bytes and interprets
nothing.

`manager_core::catalog` owns the decisions. `built_in_catalog()` is the single
definition of the compiled-in seven — the CLI and the GUI each had their own
`include_str!` list and now share this one. `refresh()` fetches each file, runs
the *existing* `ComponentManifest::parse_yaml` validation, checks that the file
name matches the declared component ID, applies the downgrade guard against what
is already held, and requires the resulting set to assemble into a
`ComponentCatalog`. Anything refused is reported as a `ManifestRejection` with a
stable machine key and leaves the previously held manifest in place.

`manager_store::catalog` is the versioned file at
`$XDG_STATE_HOME/better-os/manager-catalog.json`, schema 1, written through a
temporary file and an atomic rename, recording the source URL and the fetch
time. It is read back through the same validation that accepted it, so a
tampered cache is an absence rather than a catalog.

The CLI gained `better-manager catalog status` and `better-manager catalog
refresh` plus a `--catalog-path` override; no other command reaches the network.
The Components screen gained one row: where the list came from, how old it is, a
warning sentence when it may be behind, the count of refused manifests, and an
"Update list" button. The window refreshes once at launch on a background
thread, and `BETTER_MANAGER_OFFLINE=1` skips that.

## Verification

- `cargo fmt --all -- --check`, `cargo check --workspace`,
  `cargo test --workspace` (2,502 passed, 6 ignored), and
  `cargo clippy --workspace --all-targets -- -D warnings` all clean.
- 35 new tests: 14 in `manager-core` (full refresh, invalid manifest rejected
  alone, wrong schema version, ID mismatch, downgrade refused, equal version
  adopted, total failure over built-in and over cache, unassemblable set refused
  whole, unusable cache, future-schema cache, machine keys), 7 in
  `manager-store` (round trip, offline restart, missing, corrupt, tampered
  checksum, future schema, no temporary left behind), 4 in `manager-platform`
  (insecure base refused before any request, URL joining, missing file,
  machine keys), 4 CLI tests against the shipped binary, and 6 GUI tests over
  every degraded state, both locales, and the age phrasing.
- Two 8-second `ZED_HEADLESS=1` smokes, both silent. Offline
  (`BETTER_MANAGER_OFFLINE=1`) opened on the built-in catalog and fetched
  nothing. Online refreshed at launch and left a real cache on disk holding all
  seven manifests from `raw.githubusercontent.com`.
- The `#[ignore]`d real-network proof
  (`cargo test -p manager-core --test remote_catalog -- --ignored`) ran once
  from this machine and passed: a manager holding only the compiled-in 0.2.1
  catalog fetched and validated all seven manifests from
  `raw.githubusercontent.com/TimLai666/better-os/main`, planned
  `better-monitor` 0.2.1 for ubuntu 24.04 amd64, and downloaded the real
  16,430,640-byte `.deb`, whose bytes hashed to the declared
  `39d74a6601a5b85cf4dc78cac43687fa24037757e2765a1b854baad058c4a3c4`.

## What this ticket does not settle

The proof above is weaker than the ticket's wording, and the difference matters.
The ticket asked for a manager with the *0.2.1-placeholder* built-in catalog. On
`main` today the shipped manifests carry the real published 0.2.1 checksums,
because v0.2.1 has shipped and the values were written back. So the run proved
the whole mechanism — fetch, validate, plan, verify against a real artifact —
but it did not prove it while the built-in catalog was actually unable to verify
that release. That state only exists between a version bump and the release
being published, and the next release branch is where it can be observed.

Nothing enforces that a fetched catalog is newer than the compiled-in one in any
sense other than per-component version. A `main` that was rolled back wholesale
to an older commit, with every version lowered together, is refused component by
component by the downgrade guard; a `main` that was rolled back and re-bumped is
not distinguishable from a real release without signing.

The refresh is not scheduled. It happens at launch and on the button, so a
window left open for a week shows a week-old catalog with its age on screen and
does not go looking on its own.

## Notes

Carried over from the retired delivery status record.

Ticket 41 — the remote catalog refresh — was the previously remaining one.
Tickets 18 through 40 are done: 38 closed Issue #3's phase 3 by building the GNOME Shell
adapter the granted GJS exception allowed, so the Mac-style preset now reaches
the desktop on GNOME 46 Wayland, and 40 put `install.sh` at the repository root
so a fresh Zorin or Ubuntu machine reaches a checksum-verified Better Manager in
one command.

Ticket 41 inherits the gap the installer makes visible rather than closes. A
published `better-manager` embeds the catalog as it stood when it was built, so
the manifests it carries hold the *previous* release's checksums — the installer
puts a working Better Manager on the machine, and the components that Better
Manager then offers to install cannot be verified against the release the
installer just fetched. Fetching the catalog rather than compiling it in is what
41 is for.

Branch `ticket-41`. `AGENTS.md` has carried the same structural follow-up since
v0.2.0: a manifest checksum can only be written after its own release is public,
so the catalog compiled into release N always describes release N-1's packages,
and the follow-up ended by asking for a decision before the built-in catalog was
treated as an install path for its own release. [ADR 0013](/engineering/decisions/0013-remote-catalog-refresh.md)
takes it, and this ticket builds it.

**The seven manifest files, not a generated bundle.** Both were considered. The
files are already the source of truth a release edits and a reviewer reads, they
need no generation step that could be stale for the length of a release branch —
the exact failure this exists to remove — and a per-file fetch costs one
component when one file is bad rather than the whole catalog. The exchange is
written into the ADR: a bundle becomes the better answer the moment there is a
signature to put on it.

**A fetched manifest is untrusted in the same way a compiled-in one is.** The
whole existing `better-core` validator runs, not a lighter path, plus three
identity checks: the file name must match the component ID it declares, a lower
version than the one already held is refused, and the resulting set must still
assemble into a `ComponentCatalog`. Every refusal is individual, carries a stable
machine key, and leaves the previously held manifest in place — a bad
`better-monitor.yaml` costs Better Monitor's newer description and nothing else.

**Four states, and no fifth that means "probably fine".** Never refreshed,
refresh failed over a cache, refresh failed over the built-in catalog, and
partially refreshed. Each has its own sentence in both locales, drawn as a
warning rather than a note, because the catalog is what a person decides to
install from. A refresh that adopted nothing writes no cache, so a failure never
restamps a good file's fetch time.

**One catalog definition, not three.** `manager-cli` and `manager-gui` each
carried their own `include_str!` list of the seven manifests. Both now call
`manager_core::catalog::built_in_catalog()`, which is also the base every
refresh compares against.

Not done, and stated rather than implied. The real-network proof is weaker than
the ticket's wording: it ran against a `main` that carries the real published
0.2.1 checksums, because v0.2.1 has shipped, so it proved fetch, validate, plan,
and verify-against-a-real-artifact but not the placeholder state the ticket
describes — that state exists only between a version bump and its release, and
the next release branch is where it can be observed. Signing is unchanged and
deferred, so HTTPS plus the artifact checksum is still the whole integrity
story, and a `main` that was rolled back and re-bumped is not distinguishable
from a real release. There is no scheduled refresh: a window left open for a week
shows a week-old catalog with its age on screen and does not go looking on its
own.

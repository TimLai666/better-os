# 41 — Remote catalog refresh

**Epic:** Distribution: Better Manager installs the current release's
components without waiting for its own next build.
**User Story:** Better Manager fetches the published component manifests,
verifies them, and offers every component of the latest release — so the
built-in catalog's release-lag (a 0.2.1 manager ships 0.2.1-placeholder
manifests) stops mattering.
**Blocked by:** none (40 is independent)
**Status:** todo

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

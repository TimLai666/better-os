# ADR 0013: The refreshable component catalog

## Status

Accepted for ticket 41 on 2026-09-05.

## Context

A component manifest can only record an artifact's SHA-256 after that artifact
is published, and a binary embeds the manifests as they stood when it was
built. Those two facts together mean the catalog compiled into release N always
describes release N-1's packages. [ADR 0002](0002-release-artifact-mapping.md)
records the mechanism from the release side: the seven shipped manifests go back
to placeholder checksums when the version moves and take real values again only
after the release is public. `AGENTS.md` has carried the consequence as a
structural follow-up since v0.2.0 — a `better-manager` from a release cannot
verify that release's own packages, and only the *next* release ships a catalog
that can.

The follow-up ended with an explicit instruction: decide whether the catalog
should be fetched rather than compiled in, before treating the built-in catalog
as an install path for the release it belongs to. This decision answers it.

Everything the manager already does with manifests is reusable here.
`better-core` validates schema, targets, artifacts, dependencies, conflicts, and
lifecycle metadata, and `AGENTS.md` requires that manifests be treated as
untrusted input. A fetched manifest is untrusted in exactly the same way as a
compiled-in one, only more obviously so.

## Decision

**Better Manager fetches the seven manifest files from this repository's `main`
branch over HTTPS, validates each one with the existing validator, caches the
adopted set on disk, and falls back to the compiled-in catalog with a visible
degraded state.** Six sub-decisions make that concrete.

### What is fetched: the seven manifest files, not a generated bundle

Both were considered. A single generated `catalog.json` would be one request
instead of seven and would let the whole set be signed as one document when
signing arrives.

The seven files win today for three reasons. They are the source of truth
already: `components/manifests/*.yaml` are the files a release edits, a reviewer
reads, and the packaging scripts consume, and a bundle would be a second
representation that can disagree with them. They need no build step, so nothing
can be stale for the length of a release branch because a generation job did not
run — which is precisely the failure mode this ADR exists to remove. And a
per-file fetch degrades per file: one unpublished, truncated, or malformed
manifest costs that one component, where a bundle is all-or-nothing.

The cost is seven requests instead of one, all of them small and all of them
concurrent-free, which is paid off the UI thread.

A generated bundle becomes the better answer the moment there is a signature to
put on it. That is the exchange, and it is written down here so a later reader
does not have to rediscover it.

### From where: `raw.githubusercontent.com`, pinned in the binary

`https://raw.githubusercontent.com/TimLai666/better-os/main/components/manifests`
is a constant in `manager-platform`, not configuration. `main` is the right
branch rather than the latest tag, because `main` is where the checksums of the
newest published release are written back after the release is verified — a tag
carries the manifests as they stood *before* its own assets existed.

`BETTER_MANAGER_CATALOG_URL` overrides the base for a proof run or a fork. It
cannot weaken the scheme: a non-HTTPS base is refused before any request.

### Trust model: HTTPS plus full validation plus artifact checksums

A fetched manifest is untrusted input and gets the whole existing validator, not
a lighter path. Three checks are added on top, all of them about identity rather
than shape:

- The file name must match the component ID it declares, so a replaced file
  cannot introduce a component under a name nobody asked for.
- A manifest older than the one already held for that component is refused (see
  the downgrade guard below).
- The set that results must still assemble into a `ComponentCatalog`, so a
  missing dependency or a cycle refuses the refresh rather than producing a
  half-catalog.

What HTTPS buys is that the bytes came from GitHub unmodified in transit. What
it does not buy is that GitHub, or anyone who can write to `main`, published
what a user expected. The remaining integrity mechanism is unchanged and is the
same one a compiled-in catalog relies on: the SHA-256 in the manifest is checked
against the downloaded `.deb` before anything is installed, and a mismatch is
never retried, because the same wrong bytes will arrive again.

**Signing stays deferred**, as `AGENTS.md` requires. When a package signature
format is decided, this changes in two specific ways and no others: the catalog
becomes a signed document — the generated bundle above, most likely — verified
before any manifest in it is parsed, and an unsigned or wrongly signed catalog
is refused whole rather than degraded to. Neither change touches the seam this
ADR builds: `ManifestFetcher` returns bytes, and a signature check sits between
it and validation.

### Refresh cadence: on demand, and once at launch, never blocking

The window starts a refresh on a background thread as it opens and never waits
for it. `HttpManifestFetcher` carries a ten-second global timeout and a 256 KiB
body limit per file, so a degraded network bounds how long the catalog stays
undecided rather than how long the window takes to appear. `BETTER_MANAGER_OFFLINE=1`
skips the launch fetch entirely, which is what a headless smoke and a
disconnected machine want; it fakes no result.

The command line does *not* refresh implicitly. `better-manager catalog refresh`
is the only command that reaches the network, so a scripted `list` or `plan`
does what it says and nothing else.

There is no timer and no background polling. A catalog that refreshes when a
person opens the window or asks for it is enough for a catalog that changes a
few times a year.

### Failure behaviour: stale but honest, never silent

Four states are representable and all four are visible:

| State | What is on screen |
| --- | --- |
| Never refreshed | The compiled-in catalog, marked as possibly outdated |
| Refresh failed, cache present | The last downloaded catalog, with its age |
| Refresh failed, no cache | The compiled-in catalog, marked as unreachable |
| Partially refreshed | The adopted set, with the number of refusals |

There is deliberately no "probably fine" state. A catalog is either known to
have been fetched in full this session, or it says which way it is behind. A
refresh that adopted nothing writes no cache, so a failure never restamps a good
file's fetch time.

### The downgrade guard

A fetched manifest whose version is *lower* than the one already held for the
same component is refused and reported, never adopted. It protects against a
stale CDN edge, a reverted branch, and a mistaken force-push — cases where the
newest thing served is genuinely older than what a machine already had. An
*equal* version is adopted, because republishing a manifest at the same version
is how a corrected checksum reaches users and is not a rollback.

The guard is one-directional on purpose. It refuses; it never picks a version.

## Consequences

- A `better-manager` from release N can install release N's own components once
  it has refreshed, which is what the follow-up asked for. The compiled-in
  catalog stops being the only answer and becomes the offline fallback.
- The catalog cache is per-user state at
  `$XDG_STATE_HOME/better-os/manager-catalog.json`, versioned at schema 1, read
  back through the same validator that accepted it. A corrupt, tampered, or
  future-schema cache is ignored rather than repaired, and left on disk to be
  looked at.
- `manager-platform` owns fetching and knows nothing about manifests;
  `manager-core` owns validation, the downgrade guard, and every degraded state;
  `manager-store` owns the file. The command line and the window compose the
  same three, so neither has a private rule about what a stale catalog means.
- No default test reaches the network. `StaticManifestFetcher` is the seam, and
  the one real-network proof is `#[ignore]`d.
- Nothing here installs anything. A refresh changes the catalog, never the host.

## Deferred

- Package signing and a signed catalog bundle, as above.
- A public APT repository, which is a different distribution channel and not a
  catalog question.
- Auto-install or auto-update of components. Refresh updates what is on offer.
- Release channels. The source is `main`; a preview channel would need a second
  pinned location and a decision about which one a given install follows, and
  `ManagerSettings::release_channel` currently changes nothing here.

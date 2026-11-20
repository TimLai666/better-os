# Ticket 223 — The session runs Better OS's `cosmic-comp`, and gestures reach the gesture service

**Spec:** better-os-session.md S6, S7 ("Gestures": the contract)
**Blocked by:** 202, 222, and the owner accepting ADR 0039

## Why

C8: the gesture patch ships as a Better OS package once the rest of the
session works and upstream still offers nothing that hands gesture progress
to an outside process. Ticket 202 proved it and measured it; this ticket makes
it the session's compositor and connects Better OS's gesture service to it.

## What to build

- **First, check upstream**: whether the newest Epoch has a gesture protocol
  or setting that hands progress to an outside client (ADR 0039, F4). If it
  has, stop and bring it to the owner instead of shipping the patch.
- **The package** as ADR 0039 says: the series in
  `compositor/cosmic-comp/`, built by `packaging/build-deb.sh` in ADR 0021's
  release environments for each architecture the session ships, version
  `<upstream>+betteros<n>`, with the source tarball, the series, and the build
  script published beside it with their SHA-256 sums; the compositor's crate
  graph in `docs/third-party-licenses.md`; `copyright` naming the source
  location.
- **The pin**: `cosmic-comp` leaves the daemon's list of System76 packages
  (ticket 204's table) in the same release, and the session package depends on
  Better OS's build.
- **The gesture service's `cosmic-comp` backend** in
  `touchpad-gesture-service`: binds the gesture manager, claims the finger
  counts and kinds the person's gestures use (and releases the rest, so the
  compositor keeps its own), turns claim events into frames for
  `touchpad-gestures::ingest`, drives the workspace switch through the driver
  for the workspace gestures, performs the other actions through
  `better-actions`, reports the device of each gesture (ADR 0015), and logs as
  ticket 131's service does. The Touchpad page's released-compositor state
  (ticket 219) gives way to the normal page when the protocol is there.
- **Updates**: the compositor cannot restart under a running session; its
  package's postinst tells Better Manager, and the component page and the
  top bar say 登出後會使用更新後的合成器。 / "The updated compositor runs after you
  log out." (AGENTS.md's update rule, which this is the one session-wide
  exception to, recorded in the same change).

## Done when

- [ ] The upstream check in Notes.
- [ ] The package builds in the release environments, `verify-deb.sh` checks
      the source assets and the `copyright` line, and the license inventory
      check passes.
- [ ] Unit tests for the backend's claim set per configuration, frame
      conversion, and the driver's use; a test against the patched compositor
      nested or in the VM with the replayed recording performs the Mac-style
      preset's actions (overview, application windows, workspaces, launcher,
      desktop).
- [ ] In the VM, ticket 204's pin keeps System76's `cosmic-comp` out, and an
      upgrade from the released compositor to Better OS's and back on removal
      is clean.
- [ ] fmt, clippy on stable, and the crates' tests pass.

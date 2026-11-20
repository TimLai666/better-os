# Ticket 205 — A person installs the Better OS session, switches to it, and removes it from Better Manager

**Spec:** better-os-session.md S1, S2, S3 (flows 1 and 3)
**Blocked by:** 203, 204, and the owner's decision S2

## Why

AGENTS.md: everything is one action in Better Manager and undoing it is one
action that leaves the machine as it was. This ticket is the component the
person sees, and the two acceptance flows that prove it.

## What to build

- **Manifest** `components/manifests/better-os-session.yaml` as the spec's
  "The component" (24.04 only, `restart: logout`, `paths`, health checks
  that are the IDs the code emits), in the built-in catalog.
- **The page** in Better Manager's Apps: the description, the package list
  with its source in the install confirmation, the states and copy of the
  spec's "Switching means the next login" in both languages, Use GNOME and Use
  Better OS, Log out now, the 22.04 refusal, Waiting for log out.
- **AccountsService** from `manager-core`: read and capture the person's
  session at install (`/org/freedesktop/Accounts/User<uid>`), `SetSession` to
  `better-os` on install and Use Better OS, back to the captured one on Use
  GNOME and on removal, with the captured session gone falling back to GNOME's
  default and saying so.
- **Log out now**: GNOME's session manager under GNOME,
  `org.betteros.Session1.Logout` in the Better OS session.
- **Removal** as the spec's five steps, through ticket 204's methods; from
  inside the session, the deferred purge; other users' AccountsService
  sessions reset at purge; the person's `paths` deleted by the check box.
- **The startup pass** (`Manager::heal_user_units`' neighbour): a recorded
  session whose source or pin no longer matches is a blocking finding with
  Adopt or Repair, never a silent rewrite.

## Done when

- [ ] Unit tests for the manifest, the page model in both languages for every
      state, the AccountsService capture and restore with a fake service, and
      the removal sequence against fakes, including from inside the session and
      with another user's session open.
- [ ] Flow 1 and flow 3 of the spec pass in a disposable VM per base, from
      the built packages, with the snapshot comparison's output in
      `/home/tim/Developer/better-os-evidence/205/`; any file the session wrote
      that GNOME reads is fixed at its source (ticket 203's defaults) before
      this ticket closes.
- [ ] fmt, clippy on stable, the crates' tests, `verify-deb.sh`, and the
      license inventory check pass.

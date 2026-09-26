# Ticket 57 — Better Defaults applies a GNOME shortcut or setting itself

**Epic:** Better Defaults (Issue #10) · **User Story:** a person who applies a
component's GNOME shortcut or setting sees it applied, not "Manual action
required" · **Branch:** `ticket-57-58` · **Blocked by:** none ·
**Status:** merged, not released

## What it delivers

`DconfAdapter::write` always returns Manual action required. Ticket 29 built a
dconf write path in `touchpad-platform` — `ca.desrt.dconf.Writer.Change` over
the session bus, with the change set encoded to GLib's bytes — and ADR 0010
records it as the way a GNOME setting is written. `touchpad-platform` already
depends on `defaults-platform`, so the writer moves down into
`defaults-platform` behind an optional feature and `touchpad-platform`
re-exports it. The GNOME defaults adapters then write, read back, and report
what the read saw.

## Acceptance criteria

- [x] The writer and the GVariant encoder live in `defaults-platform`;
      `touchpad-platform` keeps its public names by re-exporting them and its
      tests pass unchanged.
- [x] The encoder supports string arrays (`as`), pinned against bytes GLib
      itself produces, like the existing encodings.
- [x] Apply writes, re-reads, and reports Applied only when the read matches.
      A mismatch or a failed write reports the failure and changes nothing
      else.
- [x] Restore of a key that had no user value resets the key rather than
      writing the default.
- [x] Tests use a private session bus or a fake writer. No test writes the
      user's real dconf database.
- [x] ADR 0009's "not adopted" status is updated to say what changed.

## What was built

### The writer moved down a crate

`touchpad-platform/src/gvariant.rs` and `touchpad-platform/src/dconf.rs` moved,
with their history, to `defaults-platform/src/gvariant.rs` and
`defaults-platform/src/dconf_writer.rs`. The writer sits behind a new
`dconf-write` feature on `defaults-platform`, which `defaults-core` turns on and
`touchpad-platform`'s own `dconf-write` feature now forwards to. Nothing moved
the other way, so there is no dependency cycle.

`touchpad-platform` keeps its public names: `touchpad_platform::gvariant` and
`touchpad_platform::dconf` are re-exports of the moved modules, and
`ChangeValue`, `Changeset`, and `ChangesetError` are still exported from its
root. The writer now returns its own `WriterError` rather than
`touchpad_platform::PlatformError`, which `defaults-platform` cannot name;
`PlatformError` converts from it into the variants of the same names, and the
one call site in `gnome.rs` converts explicitly. `touchpad-platform`'s remaining
61 unit tests and its integration tests are unchanged and pass. The 14 tests
that lived inside the two moved files moved with them, and the writer's two now
match `WriterError` instead of `PlatformError`.

### String arrays, and one change set across directories

The encoder gained `ChangeValue::TextList`, a GVariant `as`, which is what every
GNOME keybinding is. It is pinned against bytes GLib 2.80 produced for four
cases: a two-element array, an empty array, an array long enough that its own
offsets widen to two bytes, and a change set that mixes `as`, `s`, `b`, and a
reset across four directories. The empty array is pinned on purpose: `@as []`
is how GNOME disables a binding, and encoding it as a reset would bring the
default binding back.

A declaration names keys by full path, and they need not share a directory, so
`Changeset` gained `set_path` and `reset_path`. They take an absolute key under
the change set's prefix, and a prefix of `/` admits any key. That is what lets
every key a declaration names go to the service in one call, so the service
applies all of them or none.

### The adapter writes, re-reads, and reports

`DconfAdapter` writes through a `ChangesetSender`. In production that is
`SessionSender`, which connects to the session bus on the first write rather
than when the adapter is built, so inspecting and planning never open a bus
connection. A test hands it a fake, or a `DconfWriter` connected to a private
bus. `DconfAdapter::new` and a build without the feature still have no sender
and still report Manual action required, naming the keys.

The engine's rule did not change and did not need to: a write is Applied only
when the verifying read matches, a read that disagrees is Not verified, and a
failed send is Failed. None of those three updates the snapshot record, so
nothing after the capture claims a value Better Manager asked for and did not
get. A value the key already holds is not sent. A `DesktopEntry` value is
refused before anything is sent, because it has no dconf type and would read
back as text and never verify.

### One reading changed, and why

A key the user's database does not hold used to read as unknown, and so did a
database that did not exist. Both now read as *nothing set*. The old reading made
the ticket's restore criterion unreachable: an unknown current value is never
applied over, so a key the user had never set could not be captured, and so it
could never be restored by a reset. *Nothing set* is a fact about the user's own
database, and it is the one reading a restore can reproduce. The compiled
GSettings schema default is still not read, so the screen says "Nothing set"
rather than claiming which binding GNOME is using. `touchpad-platform` already
read a missing database this way.

### Tests

- Adapter, with a fake sender: every declared key in one change set, a restore
  of *nothing set* sending a reset and no value, an unchanged value not sent, a
  refused send reported as a failure with the database untouched, and a
  desktop entry and a non-dconf path both refused before anything is sent.
- Engine, with a fake sender that accepts and writes nothing: Not verified, a
  baseline snapshot, and no record claiming the value. With one that refuses:
  Failed, and still no record.
- Engine, against a real `dconf-service` on a private `dbus-daemon`. The bus is
  configured with no service directories, so nothing on it can be activated
  with the developer's environment, and both processes run with an empty
  environment whose home and XDG directories are all inside a temporary
  directory. A key that held nothing is applied, verified, and then reset on
  restore, and the test checks that the key is gone from the database rather
  than rewritten. A key the user had set is put back to their own value. When
  `dbus-daemon` or `dconf-service` is missing, the test says so and does nothing,
  like the other private-bus tests in the workspace.

### Deliberately not done

- The adapter does not probe the service before a plan is shown. A service that
  is not there is reported when the change is made, as Failed with the reason.
  Better Touchpad probes because it hides controls it cannot apply. Better
  Defaults shows a review first, and asking the bus during a read-only inspect
  would open a connection on every refresh.
- No shipped manifest declares a GNOME integration yet. `better-files.yaml`
  declares only the XDG file-manager handler, so this changes nothing a user can
  see until a manifest declares a keybinding or setting. That is ADR 0009's
  deferred catalog decision, not something this ticket decides.
- ADR 0010 still names `crates/touchpad-platform/src/gvariant.rs` and still lists
  "whether Better Defaults adopts this write path" as deferred. ADR 0009's status
  records the adoption.

### Not verified

- Nothing here has written to a real GNOME session's dconf service. The
  private-bus test runs the same `dconf-service` binary GNOME runs, on the same
  host, but it is not the user's session, so the change notification reaching a
  running GNOME Shell or GNOME Settings has not been seen.
- `SessionSender`'s own connection to the session bus is not exercised by any
  test. Every test connects to an explicit address, because the default suite
  must not reach the developer's session.


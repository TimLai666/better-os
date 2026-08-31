# Component Manifest

The schema version is currently `2`. A manifest describes an independently
versioned component without granting permission to execute its lifecycle data.

Required groups:

- identity: `schema_version`, `id`, `display_name`, `component_type`, `version`
- targets: distributions, releases, and CPU architectures
- artifacts: one release URL, asset identity, and SHA-256 checksum for every
  supported Ubuntu release and CPU architecture combination
- lifecycle: install, enable, disable, remove, and rollback descriptors

`artifacts` is a list. Each entry has `release`, `architecture`, `url`,
`release_asset`, and `sha256` fields. `release` identifies the Ubuntu build
environment, such as `22.04`; the resulting package may serve every compatible
distribution declared in `targets.distributions`. A manifest must contain
exactly one artifact entry for each combination of `targets.releases` and
`targets.architectures`.

## Release eligibility

Only manifests for components with a corresponding built and tested first-party
package are release-eligible. `components/manifests/better-files-example.yaml`
is a schema fixture for a future component. It is not a v0.1.0 release asset and
must not be treated as an installable package until that component is
implemented and included in the package matrix.

`components/manifests/better-launcher.yaml`, `better-awake.yaml`,
`better-files.yaml`, and `better-storage.yaml` are in a different position, and
one that changed with ticket 36. `packaging/build-deb.sh` now produces a package
for every one of them and `packaging/verify-deb.sh` checks each one, so the
reason their artifact checksums are still placeholders is no longer that nothing
is built. It is that ADR 0002 records the checksum of a *published* release
asset, and no release carries these components yet. A checksum copied from a
local build would name an artifact nobody can download. All four are validated
on every test run and must not be offered for installation until a release
publishes them.

`better-monitor.yaml` carries a narrower version of the same problem. Its
checksums are real, and they belong to the published v0.1.0 asset, which
contains the window alone. Ticket 36 added the session service, the command
line, and a systemd user unit to that package, so the repository now builds
different content under the same version number. Publishing the wider package
needs a version bump first.

`better-touchpad.yaml` is in the same position as the four above, and now
exists: the package matrix builds it, no release publishes it, so its checksums
are placeholders and it is validated on every test run rather than offered for
installation. It is also the manifest whose `health_checks` are the IDs the
component's own code emits — `touchpad.configuration`, `touchpad.backend`,
`touchpad.device`, `touchpad.capabilities`, `touchpad.capture`, and the
`touchpad.safe_mode` / `touchpad.integration` pair — instead of names chosen for
the catalog alone. Nothing in the schema requires that; a check name is a
declaration either way. It is worth copying because
`crates/touchpad-platform/tests/manifest.rs` can then fail when the two drift.

`better-files.yaml` is the first shipped manifest to declare
`default_integrations`, so it is also the worked example of what the group is
for: three declarations, each naming the exact keys it writes, and `restore_policy`
`captured-value` on the two that take something over. Installing the package
makes none of them true — a default is applied through Better Defaults and
restored to the value captured before the change.

Optional groups describe replacements, enhancements, dependencies, conflicts,
health checks, benchmarks, permissions, touched paths, and release notes.

A manifest may also declare its own presentation and interruption metadata:

- `summary` — one line describing what the component is for, at most 120
  characters so a component row keeps room for its version, state, and action.
- `icon` — one of `manager`, `monitor`, `files`, `launcher`, `touchpad`, or
  `generic`. The set is closed so an untrusted manifest cannot point a
  presentation layer at an arbitrary asset. It defaults to `generic`.
- `restart` — one of `none`, `application`, `logout`, or `reboot`. A manifest
  that omits it is reported as undeclared; the manager never infers a scope.
An artifact may also declare `download_size_bytes` and `required_disk_bytes`.
Both sizes must be positive when present. The manager can only preflight disk
space when the catalog declares the required size and the platform profile
provides available space; otherwise it reports that the value is unavailable.

## Default integrations

A manifest may declare which system defaults the component asks to own. Better
Manager never infers these from a component's name. The group is optional; a
manifest that declares nothing takes part in no defaults at all.

```yaml
default_integrations:
  - id: default-file-manager
    kind: application-handler
    exclusivity: exclusive
    target:
      desired:
        type: desktop_entry
        value: io.betteros.Files.desktop
      keys: [inode/directory]
    platforms: [ubuntu, zorin]
    sessions: [gnome]
    apply_adapter: xdg-default-app
    verify_adapter: xdg-effective-default
    restore_policy: captured-value
    privileges: user
    session_effect: immediate
    health_prerequisites: [installed, enabled, healthy]
```

Every declaration carries all twelve fields above. Each closed set is closed for
the same reason `icon` is: an untrusted manifest must not be able to name an
arbitrary asset, path, or command.

- `id` — stable within the component, and the key snapshots, plans, and statuses
  are filed under. Same character set as a component ID.
- `kind` — one of `application-handler`, `mime-uri-handler-group`,
  `desktop-launcher-entry`, `global-shortcut`, `input-method`, `autostart`,
  `user-service`, `tool-entry-point`, or `component-desktop-setting`.
- `exclusivity` — `exclusive` or `shared`. A second installed component holding
  an exclusive integration is reported as a conflict.
- `target.desired` — the typed value the setting should hold: `desktop_entry`,
  `text`, `text_list`, or `boolean`. There is no variant that can carry a command
  line. The type must suit the kind; a shortcut is a `text_list`, a handler is a
  `desktop_entry`.
- `target.keys` — the exact settings the adapter reads and writes, such as the
  MIME types of a handler group or a dconf path. Never a command.
- `platforms` and `sessions` — where the declaration applies. Elsewhere the
  integration reports as unavailable rather than being attempted.
- `apply_adapter` and `verify_adapter` — typed adapter IDs. A read-only adapter
  such as `xdg-effective-default` is rejected as an apply adapter, and an adapter
  that cannot serve the declared kind is rejected outright.
- `restore_policy` — `captured-value`, `leave-in-place`, or `manual-only`.
- `privileges` — `user` or `administrator`. There is no privileged executor for
  defaults, so an administrator-scope integration is reported, never attempted.
- `session_effect` — `immediate`, `sign-out`, or `restart`.
- `health_prerequisites` — any of `installed`, `enabled`, `healthy`. An unmet
  prerequisite makes the integration unavailable.

An integration kind with no production adapter reports manual action required
and executes nothing. The adapter set that ships is recorded in
[ADR 0009](decisions/0009-defaults-declarations-and-adapters.md).
`crates/better-core/tests/fixtures/every-integration-kind.yaml` declares one
integration of every kind and is the schema's coverage fixture.

## Rejections

The parser rejects unknown schema versions, empty required fields, malformed
component IDs, invalid checksums, unsafe asset names, unsupported, duplicate,
or missing artifact variants, self-conflicts, missing dependencies, and
dependency cycles. In `default_integrations` it also rejects a malformed or
repeated integration ID, an empty or repeated target key, an absent platform or
session list, a value the declared kind cannot carry, an adapter that cannot
serve the declared kind, and a read-only adapter named as the apply adapter.
Lifecycle values are data only. The current manager never executes them.

Benchmark definitions must name the baseline workload, metric, minimum desired
improvement, and maximum regression budget. A component is not considered
faster because it uses Rust or because a single run looks better.

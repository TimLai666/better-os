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

The parser rejects unknown schema versions, empty required fields, malformed
component IDs, invalid checksums, unsafe asset names, unsupported, duplicate,
or missing artifact variants, self-conflicts, missing dependencies, and
dependency cycles. Lifecycle values are data only. The current manager never
executes them.

Benchmark definitions must name the baseline workload, metric, minimum desired
improvement, and maximum regression budget. A component is not considered
faster because it uses Rust or because a single run looks better.

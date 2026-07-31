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
health checks, benchmarks, permissions, and touched paths.

The parser rejects unknown schema versions, empty required fields, malformed
component IDs, invalid checksums, unsafe asset names, unsupported, duplicate,
or missing artifact variants, self-conflicts, missing dependencies, and
dependency cycles. Lifecycle values are data only. The current manager never
executes them.

Benchmark definitions must name the baseline workload, metric, minimum desired
improvement, and maximum regression budget. A component is not considered
faster because it uses Rust or because a single run looks better.

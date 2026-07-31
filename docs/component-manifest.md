# Component Manifest

The schema version is currently `1`. A manifest describes an independently
versioned component without granting permission to execute its lifecycle data.

Required groups:

- identity: `schema_version`, `id`, `display_name`, `component_type`, `version`
- targets: distributions, releases, and CPU architectures
- artifact: release URL or asset identity and SHA-256 checksum
- lifecycle: install, enable, disable, remove, and rollback descriptors

Optional groups describe replacements, enhancements, dependencies, conflicts,
health checks, benchmarks, permissions, and touched paths.

The parser rejects unknown schema versions, empty required fields, malformed
component IDs, invalid checksums, self-conflicts, missing dependencies, and
dependency cycles. Lifecycle values are data only. The current manager never
executes them.

Benchmark definitions must name the baseline workload, metric, minimum desired
improvement, and maximum regression budget. A component is not considered
faster because it uses Rust or because a single run looks better.

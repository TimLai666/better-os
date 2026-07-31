# Security and Rollback Boundary

## Current guarantees

- CLI and GUI create plans only.
- No production code invokes `sudo`, `apt`, `dpkg`, or arbitrary manifest
  commands.
- Monitor exports redact inventory values marked sensitive.
- A transaction plan is observable and explicitly marked `dry_run`.

## Future privileged boundary

The privileged executor must be a separately reviewed process or service. It
must receive a typed plan, validate it again, restrict allowed operations, log
each step, run health checks, and preserve enough state to restore the previous
component and default configuration.

The IPC protocol is intentionally deferred. Do not infer a permanent protocol
from the current Rust traits.

## Release security

The manager verifies artifact checksums before installation. Package signing is
deferred and must be decided before public releases. The project is licensed
under GPL-3.0-or-later; third-party license notices still need review before a
public release.

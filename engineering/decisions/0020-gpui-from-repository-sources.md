# ADR 0020: GPUI and gpui-component come from their Git repositories

## Status

Accepted on 2026-07-31 (tickets 04 and 05). Written as a record on 2026-11-15
from the delivery status decision log.

## Decision

The workspace takes `gpui` and `gpui-component` as Git dependencies on their
upstream repositories, pinned by `Cargo.lock`, not from crates.io. Local and CI
checks set `RUST_FONTCONFIG_DLOPEN=1`, and CI installs the X11 and Wayland
development libraries GPUI's Linux backend links against.

## Why

GPUI is pre-1.0, and the integration path the `gpui-component` README documents
is repository dependencies. The Linux backend links fontconfig, XCB and
xkbcommon; the project states that prerequisite rather than hiding a failed GUI
link.

## Options rejected

- Published crates.io releases: not the documented integration path, and they
  lag the repository the component library is built against.

## Consequences

- Updating GPUI is a lockfile change that can move many crates at once, and the
  oldest toolchain that builds the graph is set by it (`rust-version`).
- Builds need network access to the repositories, or a populated Cargo cache.
- This holds while GPUI has no stable published release that `gpui-component`
  supports.

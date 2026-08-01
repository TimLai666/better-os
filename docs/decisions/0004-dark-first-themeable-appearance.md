# ADR 0004: Dark-First Themeable Appearance

## Status

Accepted for Better Manager.

## Decision

Better Manager opens dark. The stored appearance setting is `dark`, `light`, or
`system`, defaults to `dark`, and persists with the rest of the manager
settings. A state file written before this setting existed loads as dark.

The window applies the stored choice after the saved settings are read, not at
`gpui_component::init`, because `init` installs the component library's light
theme before any settings are available.

## Why

Issue #8 states the UI direction as dark-first but fully themeable. The first
Better Manager slice shipped light-only and recorded the dark theme as a later
decision, which left the shipped appearance contradicting the issue. Making
dark the default and keeping both explicit choices plus system-follow satisfies
"dark-first" without removing the light appearance from users who prefer it.

## Why not follow the system by default

A system default would make the first-run appearance depend on the desktop
rather than on Better OS. The issue asks for a specific identity, so the
project chooses one and lets the user opt into following the desktop.

## Deferred

The final palette, per-component accent colors, and any high-contrast
accessibility theme remain undecided. This ADR covers appearance selection, not
the design tokens themselves.

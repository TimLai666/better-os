# Ticket 234 — Sound in Better Manager

**Spec:** `design/system-settings-spec.md` section 8; ADR 0042 (proposed)
**Blocked by:** 231, and the owner's decision M1

## Why

Choosing the output, the per-application volume, and the input level are
reached in the session only through COSMIC Settings, which talks to
`cosmic-settings-daemon`'s own audio server; Better OS reads PipeWire through
the PulseAudio protocol already (ticket 157).

## What to build

- `sound-core` and `sound-platform` (section 3.1), shared with the bar's
  sound part (ticket 157): sinks, sources, ports, cards and profiles, sink
  inputs, default devices, volume and balance, the peak meter stream.
- The page of section 8; over-amplification read and written per desktop
  (`com.system76.CosmicAudio` in the session,
  `org.gnome.desktop.sound allow-volume-above-100-percent` under GNOME), and
  the bar's slider following the same key.
- First check whether `pipewire-pulse` 1.0.5 keeps a per-role volume for event
  sounds; if it does, add the alert volume card (copy written into the spec
  first); if not, say so in Notes.
- Kit: the level meter.
- If the crate links libpulse, add it to `engineering/specs/copyleft-review.md`.

## Done when

- [ ] `sound-core` tests against a fake audio server: device changes, ports
      unplugged, a Bluetooth headset switching profile, an application
      appearing and leaving, move to another sink, mute, balance on a mono
      device (not drawn), server absent.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/234/`.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.

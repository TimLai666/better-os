# Ticket 191 — Voice input on the Radeon 890M and the NPU, where it pays

**Blocked by:** 98 (for the NPU), 186

## Why

Breeze-ASR-25 has a large encoder, and others' numbers suggest the CPU needs
seconds and every core for a final pass (spec section 4.6). The owner wants the
NPU used where that pays off (ticket 99), and the integrated GPU is a second
option that needs no proprietary runtime.

## What to build

- **The GPU**: whisper.cpp's Vulkan backend on Mesa's RADV for the Radeon 890M,
  measured on the owner's machine in a container given the render node, against
  ticket 186's CPU table (wall time, CPU-seconds, package power where it can be
  read without a host change); chosen as "Automatic" only where it is faster for
  the final pass without being slower for partials.
- **The NPU**, after ticket 98 makes `/dev/accel0`, the firmware, and the runtime
  available: whisper.cpp's VitisAI plugin with AMD's runtime downloaded on the
  person's request with its licence shown first (ticket 98's rule); whether an
  encoder cache can be built for Breeze-ASR-25 with AMD's tools under its EULA,
  and if not, the same measurement for Whisper large-v3-turbo's published cache;
  the numbers against CPU and GPU in the performance file.
- **The page's "Runs on"** (spec section 7.4): the entries that measured usable,
  the automatic choice, and the NPU entry's reason while it is not.
- The word-choice model stays on the CPU unless ticket 98's small-transformer
  numbers say otherwise.

## Done when

- [ ] The GPU numbers are in `engineering/performance/input-performance.md` with
      the commands, and the automatic choice follows them in a unit-tested rule.
- [ ] The NPU numbers are there, or Notes say exactly which part of ticket 98's
      work is still missing.
- [ ] fmt, clippy on stable, the crates' tests pass.

## In the Better OS session (2026-11-21, ADR 0038 option D, decision C4)

Nothing changes.

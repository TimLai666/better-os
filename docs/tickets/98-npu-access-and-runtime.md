# Ticket 98 — The NPU is usable after installing, with nothing to type

**Blocked by:** none · **Status:** ready

## Why

The owner's ASUS Vivobook S 14 has an AMD XDNA NPU (`RyzenAI-npu4`,
firmware 1.0.0.63) and wants Better OS to use it so the CPU does less. On
2026-11-13 the kernel driver `amdxdna` was loaded and `/dev/accel0` existed,
but no program the owner runs could open it: the node is `root:render 0660`
and the owner's account is not in `render`. No NPU runtime (AMD XRT, Ryzen AI
Software) is installed, and Ubuntu's archive has none. The owner asked for
Better Manager to grant the access itself when it installs.

## What to build

- **Access, the desktop way.** A udev rule shipped by the component that needs
  the NPU (not a group change): `SUBSYSTEM=="accel", TAG+="uaccess"`, so
  systemd-logind gives the person at the active local seat an ACL on
  `/dev/accel*`, the way GPUs and webcams work. No `usermod`, no re-login, and
  a remote or second user does not get it. The rule is installed and removed
  with the package; `udevadm trigger` for the accel subsystem runs in the
  privileged install step through `manager-daemon`.
- **Runtime.** Find out, with sources, whether AMD's XRT and the Ryzen AI
  runtime (ONNX Runtime with the VitisAI execution provider) may be
  redistributed in a Better OS package, and on which Ubuntu releases and
  kernels they are supported for Strix Point. If they may, package them as a
  component; if they may not, Better Manager downloads them from AMD on the
  person's request with the licence shown first. Either way nothing lands
  outside the package manager.
- **A measurement before any feature depends on it.** One small transformer
  (the size of an input-method reranker, ~10M parameters) and one speech
  model (Whisper small or Breeze-ASR) run on CPU and on the NPU: latency,
  CPU time, and package power from RAPL, idle and under load, written into the
  ticket. Features move to the NPU only where the numbers say it is worth it.
- Better Monitor (ticket 97) shows the NPU's state and its users.

## Acceptance

- [ ] After installing in a disposable VM or container with a seat, the
      local user can open `/dev/accel0` without being in `render`; removing
      the package removes the rule.
- [ ] The redistribution answer with links to AMD's licence text.
- [ ] The measurement table on the owner's machine, CPU against NPU.

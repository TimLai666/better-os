# Ticket 52 — A person can turn on Performance mode after reading its risks

**Epic:** Safe direct-removal storage (Issue #5) · **User Story:** a person who
wants faster writes to one external disk can choose Performance mode knowing
what they give up · **Branch:** `ticket-50-52` · **Blocked by:** none ·
**Status:** implemented, not merged

## What it delivers

The storage service accepts a Performance mode request only with every key in
`PERFORMANCE_RISK_KEYS` acknowledged, and no screen presents those risks, so
nobody can turn the mode on. Issue #5 requires the trade-off explained before
activation. After this ticket, a device in Better Files' sidebar offers a
policy choice. Choosing Performance mode shows each risk in plain words, in
zh-TW and English, and the request is sent only after the person confirms each
one. Switching back to Direct Removal needs no confirmation.

## Acceptance criteria

- [ ] Every key in `PERFORMANCE_RISK_KEYS` has a zh-TW and English explanation,
      and a test fails if a key is added without one.
- [ ] The request carries exactly the keys the person confirmed. A refusal from
      the service is shown with its reason and the policy shown is the one the
      service reports afterwards, not the one requested.
- [ ] The confirmation model lives outside GPUI code and is unit-tested:
      confirm stays disabled until every risk is acknowledged, and cancelling
      sends nothing.
- [ ] With no storage service, the in-process engine is asked instead and the
      window says the choice lasts only while Better Files is open, if that is
      what the in-process engine does.
- [ ] The existing `device_policy_*` labels are used rather than duplicated.

## What was built

Each device row in the sidebar now offers the policy it does not have, as a
clickable line worded with the existing `device_policy_*` names ("Use
Performance mode" / 「改用效能模式」). It is a line rather than a button because
it has to wrap in the 236-pixel sidebar at 150%, and a button clips.

- **The confirmation is `crates/files-gui/src/policy.rs`, with no GPUI.**
  `PerformanceConfirmation` lists every key in `PERFORMANCE_RISK_KEYS` with a
  tick box, `can_confirm` is false until each one is ticked, and `request`
  carries exactly the ticked keys. `risk_text` gives each key its words in both
  languages; a test fails if a key has none.
- **The session owns the flow.** Choosing Performance mode opens the
  confirmation and sends nothing; confirming before every box is ticked does
  nothing; cancelling, or Escape, closes it and sends nothing. Choosing Direct
  Removal is sent at once. The row keeps showing the policy the last inventory
  reported: an accepted request changes nothing on screen until the storage
  layer reports the new policy, and a refusal is shown with the storage layer's
  own reason.
- **The link sends it to either backend.** `DeviceLink::request_policy` goes
  to the service's `SetPolicy` or to the in-process coordinator, and the answer
  comes back as `PolicyApplied` or `PolicyRefused`.

What the window does not say: the ticket asks for "the choice lasts only while
Better Files is open" *if* that is what the in-process engine does. It is not.
The in-process engine writes the same preference file the service reads, and a
test (`an_in_process_choice_outlives_the_window`) proves a later engine starts
from the stored choice, so no such sentence is shown.

**One finding the lead should decide on.** Performance mode does not make
anything faster today. Nothing in Better OS changes a mount option or a cache
setting, and a completed operation is flushed the same way under either
policy; the only effect of the policy is that readiness is never claimed and
Eject is required. The throughput risk is therefore worded honestly — it
explains the intended trade and says this version does not deliver the speed
yet. If that is not acceptable to ship, the alternative is to hold the choice
back until a policy change that actually speeds up writes exists, which is the
mount-option decision ticket 31 left to an ADR.

Verified: 3 model tests in `policy.rs`, 5 session tests in
`integration_tests.rs`, and 2 in `devicelink.rs` driving the in-process
coordinator through a refused and an accepted request. Not verified: the
dialog and the row control on screen. The window was opened in a nested sway
session, but this machine has no external device, so no device row exists to
draw them; the rendering compiles and passes clippy and has not been seen.

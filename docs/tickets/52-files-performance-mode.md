# Ticket 52 — A person can turn on Performance mode after reading its risks

**Epic:** Safe direct-removal storage (Issue #5) · **User Story:** a person who
wants faster writes to one external disk can choose Performance mode knowing
what they give up · **Branch:** `ticket-50-52` · **Blocked by:** none ·
**Status:** ready

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

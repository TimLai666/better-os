# Ticket 281 — The login path under injected failures: a prompt within 60 seconds, every time

**Spec:** `design/login-screen-spec.md` section 5.10 ("How it is proven"); ADR 0044 (the order, step 3)
**Blocked by:** 265, 267

## Why

The property the owner asked for is only as good as its proof. Each
failure the spec lists is injected in a VM and the property is checked.

## What to build

- In `packaging/tests/session-vm/`, on Ubuntu 24.04 and Zorin OS 18, a
  runner that boots (or logs out) once per row of the spec's injection
  table, and checks from the host: within 60 seconds of
  `better-os-greeter.service` starting, either the greeter's `ready` or the
  fallback `agetty` on `tty1` accepting input (a login typed over the serial
  console or the VM's keyboard), `plymouthd` not running, and the expected
  `BETTER_OS_LOGIN_STAGE` journal line; a screenshot of every end state.
- The normal boot measured ten times; the time to `ready` recorded.
- First with a minimal test greeter, then with `better-greeter`.

## Done when

- [ ] Every row passes on both bases, twice in a row, with evidence in
      `/home/tim/Developer/better-os-evidence/281/`.
- [ ] The spec's section 7 and the security review's "Not proven" lose the
      deadlines this proves.

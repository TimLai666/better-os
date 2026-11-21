# Ticket 263 — Both login-screen programs restrict themselves: no network, no other files, no tracing

**Spec:** `engineering/specs/login-screen-security.md` ("What each can reach", "The password")
**Blocked by:** 260

## Why

The security review's claims are rules the programs must apply and tests
must hold. This ticket writes the rules once; `better-lock` (ticket 262) and
`better-greeter` (ticket 264) apply them at start.

## What to build

- A module beside `login-screen-core`, applied at the top of `main`:
  Landlock (ABI 4) with the review's file table and TCP bind and connect
  denied; a seccomp filter refusing `socket()` for `AF_INET`, `AF_INET6`
  and `AF_PACKET`, `ptrace`, `process_vm_readv`, `process_vm_writev` and,
  once the screen is up, `execve`; `PR_SET_DUMPABLE` 0. On a kernel without
  Landlock ABI 4 the program applies what exists and logs which rule is
  missing.
- Tests against a built test binary that applies the module: a `connect()`
  from inside fails; reading a file outside the allowed table fails; a
  secret pushed through the whole path with verbose logging never appears in
  the log or in any error value; `ptrace` from another process of the same
  account is refused.
- A check that GPUI, AccessKit with Orca, and the absence of an input method
  all still work under the rules (in the sandbox desktop AGENTS.md
  describes).

## Done when

- [ ] The tests pass in CI.
- [ ] The review's "Not proven" list loses the items this ticket proves.

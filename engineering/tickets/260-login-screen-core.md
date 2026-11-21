# Ticket 260 — `login-screen-core`: the login and lock screens' states, copy and layout rules, with no GPUI

**Spec:** `design/login-screen-spec.md` sections 2 to 4, 5.1, 5.7; `engineering/specs/login-screen-security.md` ("The password")
**Blocked by:** none (the owner answered L3 (b), L4 (a) with the setting and L5 (a) on 2026-11-22)

## Why

The greeter (ticket 264) and the lock program (ticket 262) draw the same
screen. Every rule they share lives in one crate both call (AGENTS.md: "A
rule that two places need … lives in one place both call"), with tests, so
the two GPUI programs stay thin.

## What to build

- Crate `login-screen-core`, no GPUI dependency:
  - the view model: quiet, entry, chooser, session menu, password change,
    starting, failed to start, power overlay; every transition and what
    clears the field (sections 3.4, 3.9, 3.16);
  - the layout rules: wide at 800 logical pixels and wider, the card's width
    (480 at 1100 and wider, 440 below), the clock's sizes (120 / 56, and
    88 / 44 below 1100), the narrow geometry (the clock's centre from 50% to
    23%, the entry rising 48, the time `clamp(48, width / 4.8, 96)`), large
    text × 1.25 and the width at which it falls back to narrow,
    right-to-left mirroring; as numbers a GPUI view reads;
  - `Secret`: a fixed 256-byte, `mlock`ed buffer that wipes itself with
    `zeroize` on drop and on demand and implements no `Debug`, `Display`,
    `Clone` or `Serialize`; the greetd message builder that writes the
    response JSON straight into a `Secret`;
  - the wait after failures (PAM's own delay, then 1, 2, 4, 8, 16, 30 s from
    the second failure in a row) as a pure function of the failure count;
  - the account order (last login first, then name) from AccountsService
    values; the avatar decode limits (1 MiB, 1024 × 1024);
  - the session list from `.desktop` files (`Name`, `Name[zh_TW]`,
    `DesktopNames`, `Exec`; hidden and `NoDisplay` entries left out), Better
    OS first, GNOME-family entries marked for L2's caption;
  - the layout indicator's short and spoken names from an XKB layout and
    variant;
  - the known `pam_unix` prompts mapped to Better OS's copy, anything else
    passed through;
  - the lock screen's notification rows under L4 (at most three, the private
    hint, "{n} more") and the media row;
  - every string of the spec's tables in zh-TW and English, through the
    project's localization, with a test that every key exists in both.
- The date and time formatter, tested against `zh_TW`, `en_US` and the
  owner's `LC_TIME=lzh_TW`.
- `DESIGN.md`: a short "Login and lock screen" section recording the display
  sizes (the weekday at 120 and the time at 56) as this screen's one
  exception to "Nothing is larger than Title", and that the clock's text is
  white over a scrim in both appearances.
- Ticket 238's Notifications page gets the two lock-screen settings of the
  spec's section 4.2 if L4 is (a) (238 already lists "lock-screen content";
  this ticket gives it the exact copy).

## Done when

- [ ] Every rule above has unit tests, including the layout numbers at 960,
      1280, 1440 and 2560 wide and at 375 and 768 for narrow, and the
      right-to-left case.
- [ ] A test proves a `Secret`'s bytes are zero after drop and after
      `wipe()`, and that no formatting trait is implemented.
- [ ] `cargo fmt`, clippy on the newest stable and the crate's tests pass.

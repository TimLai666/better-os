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

- [x] Every rule above has unit tests, including the layout numbers at 960,
      1280, 1440 and 2560 wide and at 375 and 768 for narrow, and the
      right-to-left case.
- [x] A test proves a `Secret`'s bytes are zero after drop and after
      `wipe()`, and that no formatting trait is implemented.
- [x] `cargo fmt`, clippy on the newest stable and the crate's tests pass.

## Notes

Delivered as `crates/login-screen-core` (modules `view`, `layout`, `secret`,
`greetd`, `wait`, `accounts`, `sessions`, `keyboard`, `pam`, `power`,
`lock_content`, `clock`, `copy`). Choices made where the spec left a gap,
each now written into the spec:

- English needs a singular for the wait's last second and for one more
  notification (spec 3.9, 4.2); a login with nothing in the session menu
  says 「找不到可以啟動的工作階段。」 / "There's no session to start." (3.6).
- Large text falls back to narrow below 1010 (560 + 1.25 × 360, spec
  3.11); the compact time's line height is 52 (3.1).
- The clock's names come from `LC_TIME`, the order of the parts from the
  screen's language; `lzh_TW` uses glibc's own names and 24-hour default
  (5.7). A notification row's time follows the clock's hour cycle, not the
  mockups' `15:31` (4.2).
- The two lock-screen notification settings went into ticket 238 and
  `design/system-settings-spec.md` section 12, whose wording
  「在鎖定畫面顯示通知」 now matches this spec's 「在鎖定畫面上顯示通知」.

The status row's battery has no words of its own: `status::battery` passes
`hardware_core::charge_state::battery_look` through, whose new `spoken`
form (「電池 87%，充電中」 / "Battery 87%: Charging") names the button;
the spec's first battery copy was replaced by it (CI's `one_battery_rule`
caught the crate's own strings on pull request 36).

Unverified: the strings greetd 0.9.0 puts in an `auth_error` description
(`pam::classify_refusal` matches both `pam_strerror`'s text and the return
code's name); ticket 264 sees the real ones. Left to their tickets: drawing
all of it (262, 264), the IPC socket and PAM conversation (264, 262), the
sandbox and the log-search test for the secret (263), the on-screen
keyboard (269), the greeter's `settings.json` and `last-start.json`
records (264).

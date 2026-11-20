# Ticket 157 — The top bar's input method, network, sound, and battery

**Blocked by:** 152

## Under ADR 0038's option D (2026-11-21)

Built for the Better OS session first (ticket 152). What changes there:

- **Input method**: in the session the sources are Fcitx 5's (C4), read and
  switched through ticket 160's Fcitx 5 source; GNOME's relay
  (`InputSources`, `ActivateInputSource`, `SuppressKeyboardIndicator`) is
  GNOME's half and waits with ticket 151.
- **Network**: in the session GNOME Shell is not NetworkManager's secret agent
  and nothing else is, so the secret prompts are Better OS's own (ticket 217);
  the flyout's "Enter the password in the dialog" points at that dialog.
  Bluetooth pairing likewise (ticket 217).
- **Settings links** open COSMIC Settings' pages in the session (C9, ticket
  160).
- Sound and battery are unchanged; the battery draws ticket 170's rule.
- The headless GNOME Shell check below is done in the session instead (ticket
  201's VM, fake system services on a private bus).

## Why

`design/top-bar-spec.md` sections 6.2–6.5: the right side of the bar shows the
input method, network, sound, and battery, each with a flyout, read from the
desktop's own services and never through `manager-daemon`.

## What to build

- **Input method** (6.2): sources, the current one, and the engine's
  properties from the relay's `InputSources`; switching with
  `ActivateInputSource` and `ActivateInputProperty`; GNOME's keyboard indicator
  hidden while the bar shows it (`SuppressKeyboardIndicator`); 輸入法設定… /
  Input Method Settings….
- **Network** (6.3): NetworkManager on the system bus: primary connection,
  Wi-Fi strength, wired, VPN, airplane mode; the flyout with the Wi-Fi switch,
  access points, connecting and failed states, VPN switches, settings link;
  secrets left to GNOME's agent.
- **Sound** (6.4): the PulseAudio protocol `pipewire-pulse` serves; output and
  input volume, mute, device choice, scroll to change by 5%. Choose the crate,
  and if it links libpulse, add it to `engineering/specs/copyleft-review.md`
  and regenerate the license inventory in the same commit.
- **Battery** (6.5): UPower's display device; percentage, state words shared
  with the control center (one module both use), time remaining when UPower
  gives one, Power mode and Keep awake reusing the control center's cards'
  logic (one place), settings link.
- Each part's states from spec section 2: not there (not drawn), still reading,
  failed with Try again.

## Done when

- [ ] Unit tests for each part's model with fake services (NetworkManager,
      UPower, an audio server, the relay), every state and string in both
      languages, and no second copy of the battery words or the power-mode
      logic (a test greps for them).
- [ ] In a headless GNOME Shell 46 (sandbox rules, fake system services on a
      private bus): each icon, tooltip, and flyout matches the spec;
      screenshots in `/home/tim/Developer/better-os-evidence/157/`.
- [ ] fmt, clippy on stable, the crates' tests, and the license inventory
      check pass.

## Notes

The network, sound, and battery models are the shared crates of `design/system-settings-spec.md` section 3.1 (`network-core`, `sound-core`), used by Better Manager's pages too (tickets 232, 234): whichever of this ticket and those starts first creates them to that section. Each settings link opens the Better Manager page once it exists (section 4.3's names).

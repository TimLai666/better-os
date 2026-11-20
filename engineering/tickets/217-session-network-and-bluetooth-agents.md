# Ticket 217 — Wi-Fi passwords and Bluetooth pairing are asked for in the session

**Spec:** better-os-session.md S4 (network secrets, Bluetooth pairing row)
**Blocked by:** 157, 160

## Why

Under GNOME the shell is NetworkManager's secret agent and BlueZ's pairing
agent. In COSMIC these live in the panel's applets and COSMIC Settings (ADR
0038, read, not run), neither of which runs as an agent in the Better OS
session. Without an agent, joining a secured network from the bar or pairing
a keyboard silently fails.

## What to build

- **A NetworkManager secret agent** in `top-bar-service`, registered with
  `org.freedesktop.NetworkManager.AgentManager` in the Better OS session only:
  `GetSecrets` for Wi-Fi (WPA personal, WPA enterprise's identity and
  password), `CancelGetSecrets`, `SaveSecrets` and `DeleteSecrets` left to
  NetworkManager's own storage with the "for this user" flag; VPN secrets
  handed to the VPN plugin's own auth dialog program as NetworkManager's
  agents do; a Fluent dialog for each with zh-TW and English copy; the
  password never logged.
- **A BlueZ agent** (`org.bluez.Agent1`): `RequestPinCode`,
  `DisplayPinCode`, `RequestPasskey`, `DisplayPasskey`, `RequestConfirmation`,
  `RequestAuthorization`, `AuthorizeService`, `Cancel`, registered as the
  default agent, with dialogs and copy.
- **The bar's flyouts** (ticket 157) say 請在對話框輸入密碼。 / Enter the password
  in the dialog. as today, now pointing at Better OS's dialog in the session.

## Done when

- [ ] Unit tests for each method's state machine (cancel while asking, a
      second request while one is open, the caller leaving the bus).
- [ ] Against fake NetworkManager and BlueZ services on a private bus: each
      request answered and cancelled.
- [ ] In the VM session, joining a WPA2 network a `hostapd` in the VM's
      network offers, from the bar; screenshots in
      `/home/tim/Developer/better-os-evidence/217/`. Bluetooth pairing seen on
      the owner's machine in ticket 227.
- [ ] fmt, clippy on stable, and the crates' tests pass.
